import Foundation

nonisolated struct ClassificationResponseDTO: Codable, Equatable, Sendable {
    let success: Bool
    let model: String
    let serverVersion: Int
    let durationMs: Int
    let imageWidth: Int
    let imageHeight: Int
    let peopleCount: Int
    let people: [PersonClassification]

    init(batch: PersonClassificationBatch) {
        success = true
        model = LocalServerConfiguration.modelName
        serverVersion = LocalServerConfiguration.serverVersion
        durationMs = batch.inferenceDurationMs
        imageWidth = batch.imageWidth
        imageHeight = batch.imageHeight
        peopleCount = batch.people.count
        people = batch.people
    }
}

nonisolated struct HealthResponseDTO: Codable, Equatable, Sendable {
    let status: String
    let serverVersion: Int
    let model: String
    let modelLoaded: Bool
    let imageEncoderLoaded: Bool
    let textEncoderLoaded: Bool
    let promptEmbeddingsReady: Bool
    let humanDetector: String
    let computeUnits: String
    let error: String?
}

nonisolated struct ErrorResponseDTO: Codable, Equatable, Sendable {
    let success: Bool
    let error: String
}

nonisolated enum LocalAPIContract {
    static func isAuthorized(header: String?, token: String) -> Bool {
        header == "Bearer \(token)"
    }

    static func health(from snapshot: MobileCLIPServiceMetrics) -> HealthResponseDTO {
        let ready = snapshot.state == .ready
            && snapshot.imageEncoderLoaded
            && snapshot.textEncoderLoaded
            && snapshot.promptEmbeddingsReady
        let error: String?
        if case .failed(let message) = snapshot.state { error = message } else { error = nil }
        return HealthResponseDTO(
            status: ready ? "ok" : "error",
            serverVersion: LocalServerConfiguration.serverVersion,
            model: LocalServerConfiguration.modelName,
            modelLoaded: ready,
            imageEncoderLoaded: snapshot.imageEncoderLoaded,
            textEncoderLoaded: snapshot.textEncoderLoaded,
            promptEmbeddingsReady: snapshot.promptEmbeddingsReady,
            humanDetector: "VNDetectHumanRectanglesRequest",
            computeUnits: "all",
            error: error
        )
    }
}
