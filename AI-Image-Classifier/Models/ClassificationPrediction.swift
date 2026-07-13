import Foundation

nonisolated struct ClassificationPrediction: Codable, Sendable {
    let label: String
    let confidence: Double
}
