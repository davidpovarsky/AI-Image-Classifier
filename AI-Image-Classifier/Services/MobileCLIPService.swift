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
    nonisolated static let computeUnitFallbackOrder = ["cpuAndGPU", "cpuOnly", "all"]

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
    private let diagnostics: DiagnosticLogService
    private let ciContext = CIContext(options: [.cacheIntermediates: false])
    private var imageEncoder: MLModel?
    private var categoryEmbeddings: [PersonVisualClass: [Float]] = [:]
    private var metrics = MobileCLIPServiceMetrics()

    init(configuration: PersonClassifierConfiguration = .default, diagnostics: DiagnosticLogService = .shared) {
        self.configuration = configuration
        self.diagnostics = diagnostics
    }

    func snapshot() -> MobileCLIPServiceMetrics { metrics }

    func loadIfNeeded() async throws {
        if metrics.state == .ready { return }
        metrics.state = .loading
        metrics.stage = "promptEmbeddingsLoad"
        let started = ContinuousClock.now
        do {
            categoryEmbeddings = try Self.loadPromptEmbeddings()
            metrics.promptEmbeddingsReady = categoryEmbeddings.count == PersonVisualClass.allCases.count
            guard metrics.promptEmbeddingsReady else { throw ServiceError.promptEmbeddingsUnavailable }
            try? await diagnostics.log(level: "info", category: "model", event: "promptEmbeddingsLoaded", details: ["categories": String(categoryEmbeddings.count)])
            let modelURL = try Self.bundledURL(named: "MobileCLIP2S2ImageEncoder", extension: "mlmodelc")
            try? await diagnostics.log(level: "info", category: "modelLoad", event: "modelURLResolved", details: ["url": modelURL.path()])
            metrics.stage = "imageEncoderLoad"
            let candidates: [(MLComputeUnits, String)] = [(.cpuAndGPU, "cpuAndGPU"), (.cpuOnly, "cpuOnly"), (.all, "all")]
            for (units, name) in candidates {
                let attemptStarted = ContinuousClock.now
                try? await diagnostics.log(level: "info", category: "modelLoad", event: "attemptStarted", details: ["computeUnits": name])
                do {
                    let modelConfiguration = MLModelConfiguration()
                    modelConfiguration.computeUnits = units
                    let model = try MLModel(contentsOf: modelURL, configuration: modelConfiguration)
                    metrics.stage = "smokeTest"
                    try? await diagnostics.log(level: "info", category: "modelLoad", event: "smokeTestStarted", details: ["computeUnits": name])
                    try await smokeTest(model)
                    let attempt = Self.attempt(url: modelURL, units: name, started: attemptStarted, error: nil)
                    metrics.loadAttempts.append(attempt)
                    imageEncoder = model
                    metrics.imageEncoderLoaded = true
                    metrics.smokeTestPassed = true
                    metrics.selectedComputeUnits = name
                    try? await diagnostics.log(level: "info", category: "modelLoad", event: "smokeTestSucceeded", details: ["computeUnits": name])
                    break
                } catch {
                    let attempt = Self.attempt(url: modelURL, units: name, started: attemptStarted, error: error)
                    metrics.loadAttempts.append(attempt)
                    try? await diagnostics.log(level: "error", category: "modelLoad", event: metrics.stage == "smokeTest" ? "smokeTestFailed" : "attemptFailed", details: [
                        "computeUnits": name, "domain": attempt.errorDomain ?? "unknown",
                        "code": String(attempt.errorCode ?? 0), "description": attempt.errorDescription ?? "unknown"
                    ])
                    metrics.stage = "imageEncoderLoad"
                }
                try? await diagnostics.recordLoadAttempts(metrics.loadAttempts)
            }
            guard imageEncoder != nil, metrics.smokeTestPassed else { throw ServiceError.modelUnavailable }
            metrics.loadDurationMs = Self.milliseconds(since: started)
            metrics.modelLoadCount += 1
            metrics.stage = "ready"
            metrics.state = .ready
        } catch {
            imageEncoder = nil
            categoryEmbeddings = [:]
            metrics.imageEncoderLoaded = false
            metrics.smokeTestPassed = false
            metrics.state = .failed("MobileCLIP2-S2 assets could not be loaded: \(error.localizedDescription)")
            try? await diagnostics.recordLoadAttempts(metrics.loadAttempts)
            throw ServiceError.modelUnavailable
        }
    }

    func reload() async throws {
        imageEncoder = nil
        categoryEmbeddings = [:]
        metrics = MobileCLIPServiceMetrics()
        try await loadIfNeeded()
    }

    func classify(_ crop: PersonCrop, requestID: UUID? = nil) async throws -> PersonClassification {
        try await loadIfNeeded()
        guard let imageEncoder else { throw ServiceError.modelUnavailable }
        let started = ContinuousClock.now
        try? await diagnostics.log(level: "info", category: "inference", event: "inferenceStarted", details: [
            "requestId": requestID?.uuidString ?? "direct", "personId": crop.id.uuidString
        ])
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
        try? await diagnostics.log(level: "info", category: "inference", event: "inferenceCompleted", details: [
            "personId": crop.id.uuidString, "durationMs": String(metrics.lastInferenceDurationMs ?? 0),
            "embeddingLength": String(imageEmbedding.count), "predictedClass": classes[winningIndex].rawValue
        ])
        let finite = imageEmbedding.filter(\.isFinite)
        try? await diagnostics.appendInference([
            "requestId": requestID?.uuidString ?? "direct", "personId": crop.id.uuidString,
            "detectionSource": crop.detectionSource.rawValue,
            "personDetectionConfidence": String(crop.detectionConfidence),
            "cropWidth": String(crop.image.width), "cropHeight": String(crop.image.height),
            "modelInputWidth": "256", "modelInputHeight": "256",
            "inferenceDurationMs": String(metrics.lastInferenceDurationMs ?? 0),
            "embeddingLength": String(imageEmbedding.count),
            "embeddingNorm": String(sqrt(imageEmbedding.reduce(0) { $0 + $1 * $1 })),
            "embeddingMin": String(finite.min() ?? 0), "embeddingMax": String(finite.max() ?? 0),
            "finiteCount": String(finite.count), "nanCount": String(imageEmbedding.count - finite.count),
            "woman": String(probabilities[0]), "man": String(probabilities[1]),
            "uncertain": String(probabilities[2]), "notPerson": String(probabilities[3]),
            "predictedClass": classes[winningIndex].rawValue
        ])
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

    private func smokeTest(_ model: MLModel) async throws {
        guard let inputName = model.modelDescription.inputDescriptionsByName.first(where: { $0.value.type == .image })?.key else {
            throw ServiceError.unsupportedModelOutput
        }
        let buffer = try makeBlankPixelBuffer()
        let provider = try MLDictionaryFeatureProvider(dictionary: [inputName: MLFeatureValue(pixelBuffer: buffer)])
        let output = try await model.prediction(from: provider)
        guard let array = output.featureNames.lazy.compactMap({ output.featureValue(for: $0)?.multiArrayValue }).first else {
            throw ServiceError.unsupportedModelOutput
        }
        let embedding = Self.floatArray(from: array)
        guard let expected = categoryEmbeddings.values.first?.count,
              embedding.count == expected, EmbeddingMath.normalize(embedding) != nil else {
            throw ServiceError.numericalFailure
        }
    }

    private func makeBlankPixelBuffer() throws -> CVPixelBuffer {
        var buffer: CVPixelBuffer?
        guard CVPixelBufferCreate(kCFAllocatorDefault, 256, 256, kCVPixelFormatType_32BGRA, nil, &buffer) == kCVReturnSuccess,
              let buffer else { throw ServiceError.invalidImage }
        CVPixelBufferLockBaseAddress(buffer, [])
        if let base = CVPixelBufferGetBaseAddress(buffer) { memset(base, 0, CVPixelBufferGetDataSize(buffer)) }
        CVPixelBufferUnlockBaseAddress(buffer, [])
        return buffer
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

    nonisolated private static func attempt(
        url: URL, units: String, started: ContinuousClock.Instant, error: Error?
    ) -> ModelLoadAttempt {
        let root = error.map { $0 as NSError }
        return ModelLoadAttempt(
            timestamp: Date().ISO8601Format(), modelName: LocalServerConfiguration.modelName,
            modelURL: DiagnosticLogService.redact(url.path()), computeUnits: units,
            succeeded: error == nil, durationMs: milliseconds(since: started),
            errorDomain: root?.domain, errorCode: root?.code, errorDescription: root?.localizedDescription,
            failureReason: root?.localizedFailureReason, recoverySuggestion: root?.localizedRecoverySuggestion,
            underlyingErrors: error.map { DiagnosticLogService.flattenedErrors($0) } ?? []
        )
    }
}
