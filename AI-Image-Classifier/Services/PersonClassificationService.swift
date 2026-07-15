import CoreGraphics
import CoreImage
import ImageIO
import UIKit

actor PersonInferenceCoordinator {
    static let shared = PersonInferenceCoordinator()

    enum ServiceError: Error, Equatable {
        case invalidImage
        case modelUnavailable
        case inferenceFailed
    }

    private let humanDetectionService: HumanDetectionService
    private let cropService: PersonCropService
    private let mobileCLIPService: MobileCLIPService
    private let configuration: PersonClassifierConfiguration
    private let ciContext = CIContext(options: [.cacheIntermediates: false])

    init(
        humanDetectionService: HumanDetectionService = HumanDetectionService(),
        cropService: PersonCropService = PersonCropService(),
        mobileCLIPService: MobileCLIPService = .shared,
        configuration: PersonClassifierConfiguration = .default
    ) {
        self.humanDetectionService = humanDetectionService
        self.cropService = cropService
        self.mobileCLIPService = mobileCLIPService
        self.configuration = configuration
    }

    func prepare() async throws { try await mobileCLIPService.loadIfNeeded() }
    func reloadModel() async throws { try await mobileCLIPService.reload() }

    func modelSnapshot() async -> MobileCLIPServiceMetrics { await mobileCLIPService.snapshot() }

    func classify(imageData: Data, requestID: UUID? = nil) async throws -> PersonClassificationBatch {
        guard let image = UIImage(data: imageData), let sourceImage = image.cgImage,
              let cgImage = normalizedImage(sourceImage, orientation: image.imageOrientation.cgImagePropertyOrientation) else {
            throw ServiceError.invalidImage
        }
        let started = ContinuousClock.now
        let orientation = CGImagePropertyOrientation.up
        var detections: [HumanDetection]
        do {
            detections = try humanDetectionService.detect(in: cgImage, orientation: orientation)
            if detections.isEmpty, configuration.useFaceFallback {
                detections = try humanDetectionService.detectFaces(in: cgImage, orientation: orientation)
            }
        } catch {
            throw ServiceError.inferenceFailed
        }
        if detections.isEmpty, configuration.useWholeImageFallback {
            detections = [HumanDetection(
                id: UUID(), confidence: 0, boundingBox: CGRect(x: 0, y: 0, width: 1, height: 1),
                source: .wholeImageFallback
            )]
        }
        let crops = detections.compactMap { cropService.crop(image: cgImage, detection: $0) }
        var people: [PersonClassification] = []
        people.reserveCapacity(crops.count)
        for crop in crops {
            do {
                people.append(try await mobileCLIPService.classify(crop, requestID: requestID))
            } catch MobileCLIPService.ServiceError.modelUnavailable {
                throw ServiceError.modelUnavailable
            } catch {
                throw ServiceError.inferenceFailed
            }
        }
        return PersonClassificationBatch(
            imageWidth: cgImage.width,
            imageHeight: cgImage.height,
            people: people,
            inferenceDurationMs: Self.milliseconds(since: started)
        )
    }

    private func normalizedImage(
        _ image: CGImage,
        orientation: CGImagePropertyOrientation
    ) -> CGImage? {
        let oriented = CIImage(cgImage: image).oriented(forExifOrientation: Int32(orientation.rawValue))
        return ciContext.createCGImage(oriented, from: oriented.extent)
    }

    nonisolated private static func milliseconds(since start: ContinuousClock.Instant) -> Int {
        let duration = start.duration(to: .now)
        return Int(duration.components.seconds * 1_000)
            + Int(duration.components.attoseconds / 1_000_000_000_000_000)
    }
}
