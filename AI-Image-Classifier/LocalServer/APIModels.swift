import Foundation

nonisolated struct ClassificationResponseDTO: Codable, Sendable {
    let success: Bool
    let allowed: Bool
    let risk: String
    let confidence: Double
    let triggeredClass: String?
    let durationMs: Int
    let model: String
    let detections: [NudeDetection]
    let predictions: [ClassificationPrediction]

    init(decision: NudityPolicyDecision, durationMs: Int) {
        success = true
        allowed = decision.allowed
        risk = decision.risk
        confidence = decision.confidence
        triggeredClass = decision.triggeredClass
        self.durationMs = durationMs
        model = LocalServerConfiguration.modelName
        detections = decision.detections
        predictions = decision.detections.map {
            ClassificationPrediction(label: $0.label, confidence: $0.confidence)
        }
    }
}

nonisolated struct HealthResponseDTO: Codable, Equatable, Sendable {
    let status: String
    let serverVersion: Int
    let model: String
    let modelLoaded: Bool
    let inputSize: Int
    let computeUnits: String
    let policyVersion: Int
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
            policyVersion: NudityFilterPolicy.version,
            error: error
        )
    }
}
