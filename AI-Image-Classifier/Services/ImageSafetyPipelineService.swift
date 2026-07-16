import CoreGraphics
import CoreImage
import ImageIO
import UIKit

actor ImageSafetyPipelineService {
    static let shared = ImageSafetyPipelineService()
    nonisolated static let pipelineVersion = 1
    nonisolated static let maximumPersonCrops = 12
    nonisolated static let maximumPixelDimension = 2_048
    nonisolated static let maximumTotalPixels = 12_000_000

    enum ServiceError: Error, Equatable {
        case invalidImage
    }

    private let humanDetector: HumanDetectionService
    private let cropper: PersonCropService
    private let mobileCLIP: MobileCLIPService
    private let nudeNet: NudeNetService
    private let configuration: PersonClassifierConfiguration
    private let mapper = DetectionMappingService()
    private let merger = DetectionMergeService()
    private let ciContext = CIContext(options: [.cacheIntermediates: false])
    private var diagnostics = ImageSafetyDiagnosticsSnapshot()

    init(
        humanDetector: HumanDetectionService = HumanDetectionService(),
        mobileCLIP: MobileCLIPService = .shared,
        nudeNet: NudeNetService = .shared,
        configuration: PersonClassifierConfiguration = .default
    ) {
        self.humanDetector = humanDetector
        self.cropper = PersonCropService(horizontalPaddingFraction: 0.15, verticalPaddingFraction: 0.20)
        self.mobileCLIP = mobileCLIP
        self.nudeNet = nudeNet
        self.configuration = configuration
    }

    func snapshot() -> ImageSafetyDiagnosticsSnapshot { diagnostics }

    func readiness() async -> (mobileCLIP: MobileCLIPServiceMetrics, nudeNet: NudeNetServiceMetrics) {
        (await mobileCLIP.snapshot(), await nudeNet.snapshot())
    }

    func prepare() async {
        try? await mobileCLIP.loadIfNeeded()
        try? await nudeNet.warmUp()
    }

    func classify(imageData: Data, mimeType: String, requestID: UUID = UUID()) async throws -> ImageSafetyResponse {
        let pipelineStarted = ContinuousClock.now
        let decodeStarted = ContinuousClock.now
        guard let image = UIImage(data: imageData), let source = image.cgImage,
              let oriented = normalizedImage(source, orientation: image.imageOrientation.cgImagePropertyOrientation) else {
            throw ServiceError.invalidImage
        }
        let originalWidth = oriented.width
        let originalHeight = oriented.height
        let processed = downscaledIfNeeded(oriented)
        let scale = Double(processed.width) / Double(max(originalWidth, 1))
        let input = ImageSafetyInput(
            mimeType: mimeType, byteCount: imageData.count,
            pixelWidth: processed.width, pixelHeight: processed.height,
            orientationNormalized: true,
            originalPixelWidth: originalWidth, originalPixelHeight: originalHeight,
            processingPixelWidth: processed.width, processingPixelHeight: processed.height,
            scaleFactor: scale
        )
        let decodeReport = report(duration: elapsed(decodeStarted))

        let detectionStarted = ContinuousClock.now
        var humanDetections: [HumanDetection] = []
        var detectionError: PipelineError?
        do {
            humanDetections = try humanDetector.detect(in: processed, orientation: .up)
            if humanDetections.isEmpty {
                humanDetections = try humanDetector.detectFaces(in: processed, orientation: .up)
            }
            if humanDetections.isEmpty, configuration.useWholeImageFallback {
                humanDetections = [HumanDetection(
                    id: UUID(), confidence: 0,
                    boundingBox: CGRect(x: 0, y: 0, width: 1, height: 1),
                    source: .wholeImageFallback
                )]
            }
        } catch {
            detectionError = PipelineError(error)
        }
        let sortedDetections = humanDetections.sorted {
            let lhs = $0.boundingBox.width * $0.boundingBox.height
            let rhs = $1.boundingBox.width * $1.boundingBox.height
            return lhs == rhs ? $0.confidence > $1.confidence : lhs > rhs
        }
        let selectedDetections = Array(sortedDetections.prefix(Self.maximumPersonCrops))
        let personIDs = Dictionary(uniqueKeysWithValues: sortedDetections.enumerated().map { ($0.element.id, "person-\($0.offset + 1)") })
        let detectionEvidence = sortedDetections.compactMap { detection -> PersonDetectionEvidence? in
            guard let personID = personIDs[detection.id] else { return nil }
            return PersonDetectionEvidence(
                personId: personID, source: detection.source, confidence: detection.confidence,
                boundingBox: box(detection.boundingBox)
            )
        }
        var limitWarnings: [PipelineWarning] = []
        if sortedDetections.count > selectedDetections.count {
            limitWarnings.append(PipelineWarning(
                code: "personCropLimitReached", message: "Only the largest person detections were processed.",
                detectedPersonCount: sortedDetections.count, processedPersonCount: selectedDetections.count
            ))
        }
        var personDetectionReport = report(
            status: detectionError == nil ? .success : .failed,
            duration: elapsed(detectionStarted), warnings: limitWarnings, error: detectionError
        )
        personDetectionReport.detector = "VNDetectHumanRectanglesRequest"
        personDetectionReport.coordinateSystem = "normalized-bottom-left"
        personDetectionReport.people = detectionEvidence

        let cropStarted = ContinuousClock.now
        let crops = selectedDetections.compactMap { cropper.crop(image: processed, detection: $0) }
        let cropEvidence = crops.compactMap { crop -> PersonCropEvidence? in
            guard let personID = personIDs[crop.id] else { return nil }
            return PersonCropEvidence(
                personId: personID, cropId: "\(personID)-crop",
                sourceBoundingBox: box(crop.sourceBoundingBox),
                expandedBoundingBox: box(crop.expandedBoundingBox),
                pixelRect: PixelRectangle(
                    x: Int(crop.pixelRect.minX), y: Int(crop.pixelRect.minY),
                    width: Int(crop.pixelRect.width), height: Int(crop.pixelRect.height)
                ),
                padding: CropPadding(horizontal: 0.15, vertical: 0.20)
            )
        }
        var cropReport = report(duration: elapsed(cropStarted), warnings: limitWarnings)
        cropReport.crops = cropEvidence

        let mobileStarted = ContinuousClock.now
        var classifications: [MobileCLIPClassificationEvidence] = []
        var mobileError: PipelineError?
        for crop in crops {
            guard let personID = personIDs[crop.id] else { continue }
            do {
                let classification = try await mobileCLIP.classify(crop, requestID: requestID)
                classifications.append(MobileCLIPClassificationEvidence(
                    personId: personID, cropId: "\(personID)-crop",
                    predictedClass: classification.predictedClass,
                    confidence: classification.confidence, scores: classification.scores,
                    embedding: EmbeddingDiagnostics(dimension: 512, norm: 1, finite: true)
                ))
            } catch {
                mobileError = PipelineError(error)
                break
            }
        }
        var mobileReport = report(
            status: mobileError == nil ? .success : .failed,
            duration: elapsed(mobileStarted), error: mobileError
        )
        mobileReport.classifications = classifications
        mobileReport.model = PipelineModelDescriptor(
            name: LocalServerConfiguration.modelName, version: nil, precision: "float16",
            computeUnits: (await mobileCLIP.snapshot()).selectedComputeUnits ?? "notSelected",
            embeddingDimension: 512
        )

        let fullStarted = ContinuousClock.now
        var fullRaw: [RawNudityDetection] = []
        var fullError: PipelineError?
        do {
            let batch = try await nudeNet.detect(image: UIImage(cgImage: processed))
            fullRaw = batch.detections.enumerated().map { index, detection in
                let mapped = mapper.topLeft(fromVisionBottomLeft: detection.boundingBox)
                return RawNudityDetection(
                    detectionId: "nudity-full-\(index + 1)", rawLabel: detection.label,
                    confidence: detection.confidence, source: .fullImage,
                    boundingBox: mapped, coordinateSystem: DetectionMappingService.responseCoordinateSystem,
                    rawModelIndex: detection.classId, personId: nil, cropId: nil,
                    boundingBoxInCrop: nil, boundingBoxInOriginalImage: mapped
                )
            }
        } catch {
            fullError = PipelineError(error)
        }
        var fullReport = report(
            status: fullError == nil ? .success : .failed,
            duration: elapsed(fullStarted), error: fullError
        )
        fullReport.coordinateSystem = DetectionMappingService.responseCoordinateSystem
        fullReport.detections = fullRaw
        fullReport.model = PipelineModelDescriptor(
            name: "NudeNet320n", version: nil, precision: nil,
            computeUnits: "all", embeddingDimension: nil
        )

        let cropNudeStarted = ContinuousClock.now
        var cropRaw: [RawNudityDetection] = []
        var cropNudeError: PipelineError?
        var cropDetectionIndex = 0
        for crop in crops {
            guard let personID = personIDs[crop.id],
                  let cropInfo = cropEvidence.first(where: { $0.personId == personID }) else { continue }
            do {
                let batch = try await nudeNet.detect(image: UIImage(cgImage: crop.image))
                for detection in batch.detections {
                    cropDetectionIndex += 1
                    let inCrop = mapper.topLeft(fromVisionBottomLeft: detection.boundingBox)
                    let original = mapper.originalImageBox(
                        cropTopLeft: cropInfo.expandedBoundingBox,
                        detectionInCropBottomLeft: detection.boundingBox
                    )
                    cropRaw.append(RawNudityDetection(
                        detectionId: "nudity-crop-\(cropDetectionIndex)", rawLabel: detection.label,
                        confidence: detection.confidence, source: .personCrop,
                        boundingBox: original, coordinateSystem: DetectionMappingService.responseCoordinateSystem,
                        rawModelIndex: detection.classId, personId: personID, cropId: cropInfo.cropId,
                        boundingBoxInCrop: inCrop, boundingBoxInOriginalImage: original
                    ))
                }
            } catch {
                cropNudeError = PipelineError(error)
                continue
            }
        }
        var cropNudeReport = report(
            status: cropNudeError == nil ? .success : .failed,
            duration: elapsed(cropNudeStarted), error: cropNudeError
        )
        cropNudeReport.coordinateSystem = DetectionMappingService.responseCoordinateSystem
        cropNudeReport.detections = cropRaw
        cropNudeReport.model = fullReport.model

        let mappingStarted = ContinuousClock.now
        let allRaw = fullRaw + cropRaw
        var mappingReport = report(duration: elapsed(mappingStarted))
        mappingReport.mappedDetectionCount = allRaw.count
        mappingReport.coordinateSystem = DetectionMappingService.responseCoordinateSystem

        let mergeStarted = ContinuousClock.now
        let initiallyMerged = merger.merge(allRaw)
        let assignments = Dictionary(uniqueKeysWithValues: initiallyMerged.map {
            ($0.mergedDetectionId, merger.assignedPersonIDs(for: $0, people: detectionEvidence))
        })
        let merged = initiallyMerged.map { detection in
            MergedNudityDetection(
                mergedDetectionId: detection.mergedDetectionId,
                rawLabel: detection.rawLabel,
                confidence: detection.confidence,
                boundingBox: detection.boundingBox,
                sources: detection.sources,
                sourceDetectionIds: detection.sourceDetectionIds,
                sourceConfidences: detection.sourceConfidences,
                personIds: Array(Set(detection.personIds + assignments[detection.mergedDetectionId, default: []])).sorted(),
                merge: detection.merge
            )
        }
        let unassigned = merged.filter { assignments[$0.mergedDetectionId, default: []].isEmpty }
        var mergeReport = report(duration: elapsed(mergeStarted))
        mergeReport.rawDetectionCount = allRaw.count
        mergeReport.mergedDetectionCount = merged.count

        let people = detectionEvidence.map { detection in
            PersonSafetyEvidence(
                personId: detection.personId,
                detection: detection,
                crop: cropEvidence.first { $0.personId == detection.personId },
                mobileCLIP2: classifications.first { $0.personId == detection.personId },
                nudeNetCropDetections: cropRaw.filter { $0.personId == detection.personId },
                assignedMergedDetections: merged.filter {
                    assignments[$0.mergedDetectionId, default: []].contains(detection.personId)
                }
            )
        }
        let reports = ImageSafetyPipelineReports(
            imageDecode: decodeReport, personDetection: personDetectionReport,
            personCrops: cropReport, mobileCLIP2: mobileReport,
            nudeNetFullImage: fullReport, nudeNetPersonCrops: cropNudeReport,
            coordinateMapping: mappingReport, detectionMerge: mergeReport
        )
        let failures = [detectionError, mobileError, fullError, cropNudeError].compactMap { $0 }
        let totalDuration = elapsed(pipelineStarted)
        let pipelineStatus: PipelineStatus = failures.isEmpty ? .success : .partialSuccess
        let moduleStatuses: [String: PipelineStatus] = [
            "imageDecode": decodeReport.status, "personDetection": personDetectionReport.status,
            "personCrops": cropReport.status, "mobileCLIP2": mobileReport.status,
            "nudeNetFullImage": fullReport.status, "nudeNetPersonCrops": cropNudeReport.status,
            "coordinateMapping": mappingReport.status, "detectionMerge": mergeReport.status
        ]
        let response = ImageSafetyResponse(
            success: true, requestId: requestID.uuidString,
            serverVersion: LocalServerConfiguration.serverVersion,
            pipelineVersion: Self.pipelineVersion, timestamp: Date().ISO8601Format(), input: input,
            pipeline: ImageSafetyPipelineResult(
                status: pipelineStatus, totalDurationMs: totalDuration,
                partialFailure: !failures.isEmpty, modules: reports
            ),
            people: people,
            nudity: NudityEvidence(
                fullImageRawDetections: fullRaw, personCropRawDetections: cropRaw,
                allRawDetections: allRaw, mergedDetections: merged, unassignedDetections: unassigned
            ),
            summary: ImageSafetySummary(
                personCount: people.count, mobileCLIPClassificationCount: classifications.count,
                fullImageNudityDetectionCount: fullRaw.count, personCropNudityDetectionCount: cropRaw.count,
                mergedNudityDetectionCount: merged.count, unassignedNudityDetectionCount: unassigned.count,
                hasPersonDetections: !people.isEmpty, hasNudityDetections: !merged.isEmpty,
                highestWomanScore: classifications.map(\.scores.woman).max(),
                highestNudityConfidence: merged.map(\.confidence).max(), moduleStatuses: moduleStatuses
            ),
            warnings: limitWarnings, errors: failures
        )
        diagnostics = ImageSafetyDiagnosticsSnapshot(
            lastRequestId: response.requestId, lastStatus: pipelineStatus,
            lastTotalDurationMs: totalDuration, lastPersonCount: people.count,
            lastRawNudityCount: allRaw.count, lastMergedNudityCount: merged.count,
            lastUnassignedNudityCount: unassigned.count,
            moduleDurations: [
                "imageDecode": decodeReport.durationMs, "personDetection": personDetectionReport.durationMs,
                "personCrops": cropReport.durationMs, "mobileCLIP2": mobileReport.durationMs,
                "nudeNetFullImage": fullReport.durationMs, "nudeNetPersonCrops": cropNudeReport.durationMs,
                "coordinateMapping": mappingReport.durationMs, "detectionMerge": mergeReport.durationMs
            ]
        )
        try? await DiagnosticLogService.shared.appendImageSafety(response)
        return response
    }

    private func normalizedImage(_ image: CGImage, orientation: CGImagePropertyOrientation) -> CGImage? {
        let oriented = CIImage(cgImage: image).oriented(forExifOrientation: Int32(orientation.rawValue))
        return ciContext.createCGImage(oriented, from: oriented.extent)
    }

    private func downscaledIfNeeded(_ image: CGImage) -> CGImage {
        let width = Double(image.width)
        let height = Double(image.height)
        let dimensionScale = min(1, Double(Self.maximumPixelDimension) / max(width, height))
        let pixelScale = min(1, sqrt(Double(Self.maximumTotalPixels) / max(width * height, 1)))
        let scale = min(dimensionScale, pixelScale)
        guard scale < 1 else { return image }
        let targetWidth = max(1, Int((width * scale).rounded()))
        let targetHeight = max(1, Int((height * scale).rounded()))
        let source = CIImage(cgImage: image)
            .transformed(by: CGAffineTransform(scaleX: CGFloat(scale), y: CGFloat(scale)))
        return ciContext.createCGImage(source, from: CGRect(x: 0, y: 0, width: targetWidth, height: targetHeight)) ?? image
    }

    nonisolated private func box(_ rect: CGRect) -> NormalizedBoundingBox {
        NormalizedBoundingBox(
            x: Double(rect.minX), y: Double(rect.minY),
            width: Double(rect.width), height: Double(rect.height)
        )
    }

    nonisolated private func report(
        status: PipelineStatus = .success,
        duration: Int,
        warnings: [PipelineWarning] = [],
        error: PipelineError? = nil
    ) -> PipelineModuleReport {
        PipelineModuleReport(
            status: status, durationMs: duration, warnings: warnings, error: error,
            detector: nil, model: nil, coordinateSystem: nil, detections: nil, people: nil,
            crops: nil, classifications: nil, rawDetectionCount: nil,
            mergedDetectionCount: nil, mappedDetectionCount: nil
        )
    }

    nonisolated private func elapsed(_ start: ContinuousClock.Instant) -> Int {
        let duration = start.duration(to: .now)
        return Int(duration.components.seconds * 1_000)
            + Int(duration.components.attoseconds / 1_000_000_000_000_000)
    }
}
