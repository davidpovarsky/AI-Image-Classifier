import Foundation

nonisolated struct ClassificationResponseDTO: Codable, Sendable {
    let success: Bool
    let predictions: [ClassificationPrediction]
    let durationMs: Int
}

nonisolated struct HealthResponseDTO: Codable, Sendable {
    let status: String
    let model: String
    let serverVersion: Int
}

nonisolated struct ErrorResponseDTO: Codable, Sendable {
    let success: Bool
    let error: String
}
