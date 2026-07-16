import Foundation

nonisolated enum PipelineStatus: String, Codable, Sendable {
    case success
    case partialSuccess
    case failed
    case skipped
}

nonisolated struct PipelineError: Codable, Equatable, Sendable {
    let domain: String
    let code: Int
    let message: String
    let underlyingErrors: [DiagnosticError]

    init(_ error: Error) {
        let value = error as NSError
        domain = value.domain
        code = value.code
        message = value.localizedDescription
        underlyingErrors = DiagnosticLogService.flattenedErrors(error)
    }
}

nonisolated struct PipelineWarning: Codable, Equatable, Sendable {
    let code: String
    let message: String
    let detectedPersonCount: Int?
    let processedPersonCount: Int?
}

nonisolated struct ImageSafetyInput: Codable, Equatable, Sendable {
    let mimeType: String
    let byteCount: Int
    let pixelWidth: Int
    let pixelHeight: Int
    let orientationNormalized: Bool
    let originalPixelWidth: Int
    let originalPixelHeight: Int
    let processingPixelWidth: Int
    let processingPixelHeight: Int
    let scaleFactor: Double
}

nonisolated struct PixelRectangle: Codable, Equatable, Sendable {
    let x: Int
    let y: Int
    let width: Int
    let height: Int
}

nonisolated struct CropPadding: Codable, Equatable, Sendable {
    let horizontal: Double
    let vertical: Double
}

nonisolated struct PersonDetectionEvidence: Codable, Equatable, Sendable {
    let personId: String
    let source: DetectionSource
    let confidence: Float
    /// The module preserves Vision's normalized lower-left coordinates.
    let boundingBox: NormalizedBoundingBox
}

nonisolated struct PersonCropEvidence: Codable, Equatable, Sendable {
    let personId: String
    let cropId: String
    let sourceBoundingBox: NormalizedBoundingBox
    let expandedBoundingBox: NormalizedBoundingBox
    let pixelRect: PixelRectangle
    let padding: CropPadding
}

nonisolated struct EmbeddingDiagnostics: Codable, Equatable, Sendable {
    let included = false
    let dimension: Int
    let norm: Float
    let finite: Bool
}

nonisolated struct MobileCLIPClassificationEvidence: Codable, Equatable, Sendable {
    let personId: String
    let cropId: String
    let predictedClass: PersonVisualClass
    let confidence: Float
    let scores: PersonScores
    let embedding: EmbeddingDiagnostics
}

nonisolated struct PipelineModelDescriptor: Codable, Equatable, Sendable {
    let name: String
    let version: String?
    let precision: String?
    let computeUnits: String
    let embeddingDimension: Int?
}

nonisolated enum NudityDetectionSource: String, Codable, Hashable, Sendable {
    case fullImage
    case personCrop
}

nonisolated struct RawNudityDetection: Codable, Equatable, Sendable {
    let detectionId: String
    let rawLabel: String
    let confidence: Double
    let source: NudityDetectionSource
    /// Always normalized top-left for aggregation and client use.
    let boundingBox: NormalizedBoundingBox
    let coordinateSystem: String
    let rawModelIndex: Int
    let personId: String?
    let cropId: String?
    let boundingBoxInCrop: NormalizedBoundingBox?
    let boundingBoxInOriginalImage: NormalizedBoundingBox
}

nonisolated struct MergeEvidence: Codable, Equatable, Sendable {
    let algorithm: String
    let threshold: Double
    let maximumIoU: Double
}

nonisolated struct MergedNudityDetection: Codable, Equatable, Sendable {
    let mergedDetectionId: String
    let rawLabel: String
    let confidence: Double
    let boundingBox: NormalizedBoundingBox
    let sources: [NudityDetectionSource]
    let sourceDetectionIds: [String]
    let sourceConfidences: [Double]
    let personIds: [String]
    let merge: MergeEvidence
}

nonisolated struct PersonSafetyEvidence: Codable, Equatable, Sendable {
    let personId: String
    let detection: PersonDetectionEvidence
    let crop: PersonCropEvidence?
    let mobileCLIP2: MobileCLIPClassificationEvidence?
    let nudeNetCropDetections: [RawNudityDetection]
    let assignedMergedDetections: [MergedNudityDetection]
}

nonisolated struct PipelineModuleReport: Codable, Equatable, Sendable {
    let status: PipelineStatus
    let durationMs: Int
    let warnings: [PipelineWarning]
    let error: PipelineError?
    var detector: String?
    var model: PipelineModelDescriptor?
    var coordinateSystem: String?
    var detections: [RawNudityDetection]?
    var people: [PersonDetectionEvidence]?
    var crops: [PersonCropEvidence]?
    var classifications: [MobileCLIPClassificationEvidence]?
    var rawDetectionCount: Int?
    var mergedDetectionCount: Int?
    var mappedDetectionCount: Int?
}

nonisolated struct ImageSafetyPipelineReports: Codable, Equatable, Sendable {
    let imageDecode: PipelineModuleReport
    let personDetection: PipelineModuleReport
    let personCrops: PipelineModuleReport
    let mobileCLIP2: PipelineModuleReport
    let nudeNetFullImage: PipelineModuleReport
    let nudeNetPersonCrops: PipelineModuleReport
    let coordinateMapping: PipelineModuleReport
    let detectionMerge: PipelineModuleReport
}

nonisolated struct ImageSafetyPipelineResult: Codable, Equatable, Sendable {
    let status: PipelineStatus
    let totalDurationMs: Int
    let partialFailure: Bool
    let modules: ImageSafetyPipelineReports
}

nonisolated struct NudityEvidence: Codable, Equatable, Sendable {
    let fullImageRawDetections: [RawNudityDetection]
    let personCropRawDetections: [RawNudityDetection]
    let allRawDetections: [RawNudityDetection]
    let mergedDetections: [MergedNudityDetection]
    let unassignedDetections: [MergedNudityDetection]
}

nonisolated struct ImageSafetySummary: Codable, Equatable, Sendable {
    let personCount: Int
    let mobileCLIPClassificationCount: Int
    let fullImageNudityDetectionCount: Int
    let personCropNudityDetectionCount: Int
    let mergedNudityDetectionCount: Int
    let unassignedNudityDetectionCount: Int
    let hasPersonDetections: Bool
    let hasNudityDetections: Bool
    let highestWomanScore: Float?
    let highestNudityConfidence: Double?
    let moduleStatuses: [String: PipelineStatus]
}

nonisolated struct ImageSafetyResponse: Codable, Equatable, Sendable {
    let success: Bool
    let requestId: String
    let serverVersion: Int
    let pipelineVersion: Int
    let timestamp: String
    let input: ImageSafetyInput
    let pipeline: ImageSafetyPipelineResult
    let people: [PersonSafetyEvidence]
    let nudity: NudityEvidence
    let summary: ImageSafetySummary
    let warnings: [PipelineWarning]
    let errors: [PipelineError]
}

nonisolated struct ImageSafetyDiagnosticsSnapshot: Codable, Equatable, Sendable {
    var lastRequestId: String?
    var lastStatus: PipelineStatus?
    var lastTotalDurationMs: Int?
    var lastPersonCount = 0
    var lastRawNudityCount = 0
    var lastMergedNudityCount = 0
    var lastUnassignedNudityCount = 0
    var moduleDurations: [String: Int] = [:]
}
