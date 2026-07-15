import CoreImage
import CoreML
import Foundation

nonisolated enum EmbeddingMath {
    static func normalize(_ vector: [Float]) -> [Float]? {
        guard !vector.isEmpty, vector.allSatisfy(\.isFinite) else { return nil }
        let squaredNorm = vector.reduce(Float.zero) { $0 + $1 * $1 }
        guard squaredNorm.isFinite, squaredNorm > .leastNonzeroMagnitude else { return nil }
        let norm = sqrt(squaredNorm)
        return vector.map { $0 / norm }
    }

    static func categoryEmbedding(_ embeddings: [[Float]]) -> [Float]? {
        let normalized = embeddings.compactMap(normalize)
        guard normalized.count == embeddings.count, let width = normalized.first?.count,
              normalized.allSatisfy({ $0.count == width }) else { return nil }
        var mean = [Float](repeating: 0, count: width)
        for vector in normalized {
            for index in vector.indices { mean[index] += vector[index] }
        }
        let count = Float(normalized.count)
        return normalize(mean.map { $0 / count })
    }

    static func cosineSimilarity(_ lhs: [Float], _ rhs: [Float]) -> Float? {
        guard lhs.count == rhs.count, let a = normalize(lhs), let b = normalize(rhs) else { return nil }
        return zip(a, b).reduce(Float.zero) { $0 + $1.0 * $1.1 }
    }

    static func softmax(_ values: [Float]) -> [Float]? {
        guard !values.isEmpty, values.allSatisfy(\.isFinite), let maximum = values.max() else { return nil }
        let exponentials = values.map { exp($0 - maximum) }
        let total = exponentials.reduce(0, +)
        guard total.isFinite, total > 0 else { return nil }
        return exponentials.map { $0 / total }
    }
}

actor MobileCLIPService {
    static let shared = MobileCLIPService()

    enum ServiceError: Error, Equatable {
        case modelUnavailable
        case promptEmbeddingsUnavailable
        case invalidImage
        case unsupportedModelOutput
        case numericalFailure
    }

    private struct PromptEmbeddingFile: Decodable {
        let model: String
        let promptConfigurationHash: String
        let embeddings: [String: [Float]]
    }

    private let configuration: PersonClassifierConfiguration
    private let ciContext = CIContext(options: [.cacheIntermediates: false])
    private var imageEncoder: MLModel?
    private var textEncoder: MLModel?
    private var categoryEmbeddings: [PersonVisualClass: [Float]] = [:]
    private var metrics = MobileCLIPServiceMetrics()

    init(configuration: PersonClassifierConfiguration = .default) {
        self.configuration = configuration
    }

    func snapshot() -> MobileCLIPServiceMetrics { metrics }

    func loadIfNeeded() async throws {
        if metrics.state == .ready { return }
        metrics.state = .loading
        let started = ContinuousClock.now
        do {
            let modelConfiguration = MLModelConfiguration()
            modelConfiguration.computeUnits = .all
            imageEncoder = try MLModel(
                contentsOf: Self.bundledURL(named: "MobileCLIP2S2ImageEncoder", extension: "mlmodelc"),
                configuration: modelConfiguration
            )
            metrics.imageEncoderLoaded = true
            textEncoder = try MLModel(
                contentsOf: Self.bundledURL(named: "MobileCLIP2S2TextEncoder", extension: "mlmodelc"),
                configuration: modelConfiguration
            )
            metrics.textEncoderLoaded = true
            categoryEmbeddings = try Self.loadPromptEmbeddings()
            metrics.promptEmbeddingsReady = categoryEmbeddings.count == PersonVisualClass.allCases.count
            guard metrics.promptEmbeddingsReady else { throw ServiceError.promptEmbeddingsUnavailable }
            metrics.loadDurationMs = Self.milliseconds(since: started)
            metrics.modelLoadCount += 1
            metrics.state = .ready
        } catch {
            imageEncoder = nil
            textEncoder = nil
            categoryEmbeddings = [:]
            metrics.imageEncoderLoaded = false
            metrics.textEncoderLoaded = false
            metrics.promptEmbeddingsReady = false
            metrics.state = .failed("MobileCLIP2-S2 assets could not be loaded: \(error.localizedDescription)")
            throw ServiceError.modelUnavailable
        }
    }

    func classify(_ crop: PersonCrop) async throws -> PersonClassification {
        try await loadIfNeeded()
        guard let imageEncoder else { throw ServiceError.modelUnavailable }
        let started = ContinuousClock.now
        let pixelBuffer = try makePixelBuffer(from: crop.image)
        guard let inputName = imageEncoder.modelDescription.inputDescriptionsByName.first(where: {
            $0.value.type == .image
        })?.key else { throw ServiceError.unsupportedModelOutput }
        let provider = try MLDictionaryFeatureProvider(dictionary: [
            inputName: MLFeatureValue(pixelBuffer: pixelBuffer)
        ])
        let output = try await imageEncoder.prediction(from: provider)
        guard let feature = output.featureNames.lazy.compactMap({ output.featureValue(for: $0) })
            .first(where: { $0.type == .multiArray }),
              let multiArray = feature.multiArrayValue,
              let imageEmbedding = EmbeddingMath.normalize(Self.floatArray(from: multiArray)) else {
            throw ServiceError.unsupportedModelOutput
        }

        let classes = PersonVisualClass.allCases
        let logits = try classes.map { visualClass -> Float in
            guard let category = categoryEmbeddings[visualClass],
                  let similarity = EmbeddingMath.cosineSimilarity(imageEmbedding, category) else {
                throw ServiceError.numericalFailure
            }
            return similarity * configuration.logitScale
        }
        guard let probabilities = EmbeddingMath.softmax(logits),
              let winningIndex = probabilities.indices.max(by: { probabilities[$0] < probabilities[$1] }) else {
            throw ServiceError.numericalFailure
        }
        metrics.lastInferenceDurationMs = Self.milliseconds(since: started)
        return PersonClassification(
            id: crop.id,
            detectionSource: crop.detectionSource,
            personDetectionConfidence: crop.detectionConfidence,
            boundingBox: NormalizedBoundingBox(
                x: Double(crop.sourceBoundingBox.minX),
                y: Double(crop.sourceBoundingBox.minY),
                width: Double(crop.sourceBoundingBox.width),
                height: Double(crop.sourceBoundingBox.height)
            ),
            predictedClass: classes[winningIndex],
            confidence: probabilities[winningIndex],
            scores: PersonScores(
                woman: probabilities[0],
                man: probabilities[1],
                uncertain: probabilities[2],
                notPerson: probabilities[3]
            )
        )
    }

    private func makePixelBuffer(from image: CGImage) throws -> CVPixelBuffer {
        let size = 256
        var pixelBuffer: CVPixelBuffer?
        let attributes: [CFString: Any] = [
            kCVPixelBufferCGImageCompatibilityKey: true,
            kCVPixelBufferCGBitmapContextCompatibilityKey: true,
            kCVPixelBufferMetalCompatibilityKey: true
        ]
        guard CVPixelBufferCreate(
            kCFAllocatorDefault, size, size, kCVPixelFormatType_32BGRA,
            attributes as CFDictionary, &pixelBuffer
        ) == kCVReturnSuccess, let pixelBuffer else { throw ServiceError.invalidImage }
        let source = CIImage(cgImage: image)
        let squareLength = min(source.extent.width, source.extent.height)
        let square = source.cropped(to: CGRect(
            x: source.extent.midX - squareLength / 2,
            y: source.extent.midY - squareLength / 2,
            width: squareLength,
            height: squareLength
        ))
        let scale = CGFloat(size) / squareLength
        let resized = square
            .transformed(by: CGAffineTransform(translationX: -square.extent.minX, y: -square.extent.minY))
            .transformed(by: CGAffineTransform(scaleX: scale, y: scale))
        ciContext.render(resized, to: pixelBuffer, bounds: CGRect(x: 0, y: 0, width: size, height: size), colorSpace: CGColorSpaceCreateDeviceRGB())
        return pixelBuffer
    }

    nonisolated private static func loadPromptEmbeddings() throws -> [PersonVisualClass: [Float]] {
        let url = try bundledURL(named: "MobileCLIP2S2PromptEmbeddings", extension: "json")
        let file = try JSONDecoder().decode(PromptEmbeddingFile.self, from: Data(contentsOf: url))
        guard file.model == LocalServerConfiguration.modelName,
              file.promptConfigurationHash == MobileCLIPPromptConfiguration.configurationHash else {
            throw ServiceError.promptEmbeddingsUnavailable
        }
        return try Dictionary(uniqueKeysWithValues: PersonVisualClass.allCases.map { visualClass in
            guard let embedding = file.embeddings[visualClass.rawValue],
                  let normalized = EmbeddingMath.normalize(embedding) else {
                throw ServiceError.promptEmbeddingsUnavailable
            }
            return (visualClass, normalized)
        })
    }

    nonisolated private static func bundledURL(named name: String, extension fileExtension: String) throws -> URL {
        guard let url = Bundle.main.url(forResource: name, withExtension: fileExtension) else {
            throw ServiceError.modelUnavailable
        }
        return url
    }

    nonisolated private static func floatArray(from array: MLMultiArray) -> [Float] {
        (0..<array.count).map { array[$0].floatValue }
    }

    nonisolated private static func milliseconds(since start: ContinuousClock.Instant) -> Int {
        let duration = start.duration(to: .now)
        return Int(duration.components.seconds * 1_000)
            + Int(duration.components.attoseconds / 1_000_000_000_000_000)
    }
}
