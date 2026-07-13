import CoreImage
import CoreML
import ImageIO
import OSLog
import UIKit
import Vision

nonisolated enum NudeNetModelState: Equatable, Sendable {
    case notLoaded
    case loading
    case warming
    case ready
    case failed(String)
}

nonisolated struct NudeNetServiceMetrics: Equatable, Sendable {
    var state: NudeNetModelState = .notLoaded
    var loadDurationMs: Int?
    var warmUpDurationMs: Int?
    var lastInferenceDurationMs: Int?
    var modelLoadCount = 0
}

actor NudeNetService {
    static let shared = NudeNetService()

    enum ServiceError: Error, Equatable {
        case invalidImage
        case modelUnavailable
        case modelOutputUnsupported
        case inferenceFailed
    }

    typealias ModelLoader = @Sendable () throws -> VNCoreMLModel

    private static let logger = Logger(
        subsystem: Bundle.main.bundleIdentifier ?? "AI-Image-Classifier",
        category: "model"
    )

    private let modelLoader: ModelLoader
    private var model: VNCoreMLModel?
    private var didWarmUp = false
    private var metrics = NudeNetServiceMetrics()

    init(modelLoader: @escaping ModelLoader = NudeNetService.loadBundledModel) {
        self.modelLoader = modelLoader
    }

    func snapshot() -> NudeNetServiceMetrics { metrics }

    func loadIfNeeded() async throws {
        if model != nil { return }
        metrics.state = .loading
        let started = ContinuousClock.now
        do {
            model = try modelLoader()
            metrics.loadDurationMs = Self.milliseconds(since: started)
            metrics.modelLoadCount += 1
            let loadDurationMs = metrics.loadDurationMs ?? 0
            Self.logger.info("NudeNet model loaded in \(loadDurationMs, privacy: .public) ms")
        } catch {
            metrics.state = .failed("The NudeNet model could not be loaded.")
            Self.logger.error("NudeNet model loading failed")
            throw ServiceError.modelUnavailable
        }
    }

    func warmUp() async throws {
        if didWarmUp { return }
        try await loadIfNeeded()
        guard let model else { throw ServiceError.modelUnavailable }
        metrics.state = .warming
        let started = ContinuousClock.now
        do {
            var pixelBuffer: CVPixelBuffer?
            let attributes: [CFString: Any] = [
                kCVPixelBufferCGImageCompatibilityKey: true,
                kCVPixelBufferCGBitmapContextCompatibilityKey: true
            ]
            guard CVPixelBufferCreate(
                kCFAllocatorDefault,
                320,
                320,
                kCVPixelFormatType_32BGRA,
                attributes as CFDictionary,
                &pixelBuffer
            ) == kCVReturnSuccess, let pixelBuffer else {
                throw ServiceError.inferenceFailed
            }
            let request = VNCoreMLRequest(model: model)
            request.imageCropAndScaleOption = .scaleFit
            try VNImageRequestHandler(cvPixelBuffer: pixelBuffer).perform([request])
            metrics.warmUpDurationMs = Self.milliseconds(since: started)
            metrics.state = .ready
            didWarmUp = true
            let warmUpDurationMs = metrics.warmUpDurationMs ?? 0
            Self.logger.info("NudeNet warm-up completed in \(warmUpDurationMs, privacy: .public) ms")
        } catch {
            metrics.state = .failed("The NudeNet model could not be warmed up.")
            Self.logger.error("NudeNet model warm-up failed")
            throw error
        }
    }

    func detect(imageData: Data) async throws -> NudeDetectionBatch {
        guard let image = UIImage(data: imageData) else { throw ServiceError.invalidImage }
        return try await detect(image: image)
    }

    func detect(image: UIImage) async throws -> NudeDetectionBatch {
        try await warmUp()
        guard let model, let cgImage = image.cgImage else { throw ServiceError.invalidImage }

        let started = ContinuousClock.now
        let request = VNCoreMLRequest(model: model)
        request.imageCropAndScaleOption = .scaleFit
        do {
            try autoreleasepool {
                let handler = VNImageRequestHandler(
                    cgImage: cgImage,
                    orientation: image.imageOrientation.cgImagePropertyOrientation
                )
                try handler.perform([request])
            }
        } catch {
            Self.logger.error("NudeNet inference failed")
            throw ServiceError.inferenceFailed
        }

        guard let observations = request.results as? [VNRecognizedObjectObservation] else {
            throw ServiceError.modelOutputUnsupported
        }
        let durationMs = Self.milliseconds(since: started)
        metrics.lastInferenceDurationMs = durationMs
        let detections = observations.compactMap { observation -> NudeDetection? in
            guard let label = observation.labels.first,
                  let classId = NudeNetLabels.classId(for: label.identifier) else { return nil }
            let box = observation.boundingBox
            return NudeDetection(
                classId: classId,
                label: label.identifier,
                confidence: Double(label.confidence),
                boundingBox: NormalizedBoundingBox(
                    x: Double(box.origin.x),
                    y: Double(box.origin.y),
                    width: Double(box.width),
                    height: Double(box.height)
                )
            )
        }.sorted { $0.confidence > $1.confidence }
        return NudeDetectionBatch(detections: detections, inferenceDurationMs: durationMs)
    }

    nonisolated private static func loadBundledModel() throws -> VNCoreMLModel {
        let configuration = MLModelConfiguration()
        configuration.computeUnits = .all
        guard let url = Bundle.main.url(forResource: "NudeNet320n", withExtension: "mlmodelc") else {
            throw ServiceError.modelUnavailable
        }
        return try VNCoreMLModel(for: MLModel(contentsOf: url, configuration: configuration))
    }

    nonisolated private static func milliseconds(since start: ContinuousClock.Instant) -> Int {
        let duration = start.duration(to: .now)
        return Int(duration.components.seconds * 1_000)
            + Int(duration.components.attoseconds / 1_000_000_000_000_000)
    }
}

nonisolated extension UIImage.Orientation {
    var cgImagePropertyOrientation: CGImagePropertyOrientation {
        switch self {
        case .up: .up
        case .upMirrored: .upMirrored
        case .down: .down
        case .downMirrored: .downMirrored
        case .left: .left
        case .leftMirrored: .leftMirrored
        case .right: .right
        case .rightMirrored: .rightMirrored
        @unknown default: .up
        }
    }
}
