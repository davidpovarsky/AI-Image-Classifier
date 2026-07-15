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
    var promptEmbeddingsReady = false
    var selectedComputeUnits: String?
    var stage = "notLoaded"
    var smokeTestPassed = false
    var loadAttempts: [ModelLoadAttempt] = []
    var loadDurationMs: Int?
    var lastInferenceDurationMs: Int?
    var modelLoadCount = 0
}

nonisolated struct DiagnosticError: Codable, Equatable, Sendable {
    let domain: String
    let code: Int
    let description: String
    let failureReason: String?
    let recoverySuggestion: String?
    let userInfo: [String: String]
}

nonisolated struct ModelLoadAttempt: Codable, Equatable, Sendable, Identifiable {
    var id: String { "\(timestamp)-\(computeUnits)" }
    let timestamp: String
    let modelName: String
    let modelURL: String
    let computeUnits: String
    let succeeded: Bool
    let durationMs: Int
    let errorDomain: String?
    let errorCode: Int?
    let errorDescription: String?
    let failureReason: String?
    let recoverySuggestion: String?
    let underlyingErrors: [DiagnosticError]
}

nonisolated enum InferenceMode: Sendable {
    case mobileCLIPPersonClassifier
    case nudeNetLegacy
}
