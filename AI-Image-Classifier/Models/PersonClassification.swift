import CoreGraphics
import Foundation

nonisolated enum DetectionSource: String, Codable, Sendable {
    case humanRectangle
    case faceFallback
    case wholeImageFallback
}

/// Vision coordinates are normalized and use a lower-left origin.
nonisolated struct HumanDetection: Equatable, Sendable {
    let id: UUID
    let confidence: Float
    let boundingBox: CGRect
    let source: DetectionSource
}

nonisolated struct PersonCrop: @unchecked Sendable {
    let id: UUID
    let image: CGImage
    let sourceBoundingBox: CGRect
    let detectionConfidence: Float
    let detectionSource: DetectionSource
}

nonisolated enum PersonVisualClass: String, Codable, CaseIterable, Sendable {
    case woman
    case man
    case uncertain
    case notPerson
}

nonisolated struct PersonScores: Codable, Equatable, Sendable {
    let woman: Float
    let man: Float
    let uncertain: Float
    let notPerson: Float
}

nonisolated struct PersonClassification: Codable, Equatable, Sendable {
    let id: UUID
    let detectionSource: DetectionSource
    let personDetectionConfidence: Float
    let boundingBox: NormalizedBoundingBox
    let predictedClass: PersonVisualClass
    let confidence: Float
    let scores: PersonScores

    enum CodingKeys: String, CodingKey {
        case id, detectionSource, personDetectionConfidence
        case boundingBox = "box"
        case predictedClass, confidence, scores
    }
}

nonisolated struct PersonClassificationBatch: Equatable, Sendable {
    let imageWidth: Int
    let imageHeight: Int
    let people: [PersonClassification]
    let inferenceDurationMs: Int
}

nonisolated enum MobileCLIPModelState: Equatable, Sendable {
    case notLoaded
    case loading
    case ready
    case failed(String)
}

nonisolated struct MobileCLIPServiceMetrics: Equatable, Sendable {
    var state: MobileCLIPModelState = .notLoaded
    var imageEncoderLoaded = false
    var textEncoderLoaded = false
    var promptEmbeddingsReady = false
    var loadDurationMs: Int?
    var lastInferenceDurationMs: Int?
    var modelLoadCount = 0
}

nonisolated enum InferenceMode: Sendable {
    case mobileCLIPPersonClassifier
    case nudeNetLegacy
}
