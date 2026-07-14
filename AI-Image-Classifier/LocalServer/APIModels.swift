import Foundation

nonisolated struct ClassificationResponseDTO: Codable, Equatable, Sendable {
    let success: Bool
    let model: String
    let durationMs: Int
    let detections: [NudeDetection]

    init(batch: NudeDetectionBatch) {
        success = true
        model = LocalServerConfiguration.modelName
        durationMs = batch.inferenceDurationMs
        detections = batch.detections
    }
}

nonisolated struct HealthResponseDTO: Codable, Equatable, Sendable {
    let status: String
    let serverVersion: Int
    let model: String
    let modelLoaded: Bool
    let inputSize: Int
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

    static func health(from snapshot: NudeNetServiceMetrics) -> HealthResponseDTO {
        let ready = snapshot.state == .ready
        let error: String?
        if case .failed(let message) = snapshot.state { error = message } else { error = nil }
        return HealthResponseDTO(
            status: ready ? "ok" : "unavailable",
            serverVersion: LocalServerConfiguration.serverVersion,
            model: LocalServerConfiguration.modelName,
            modelLoaded: ready,
            inputSize: 320,
            computeUnits: "all",
            error: error
        )
    }
}
