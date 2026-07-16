import Foundation

nonisolated struct LocalServerConfiguration: Sendable {
    static let host = "127.0.0.1"
    static let modelName = "MobileCLIP2-S2"
    static let serverVersion = 6
    static let inferenceMode: InferenceMode = .mobileCLIPPersonClassifier

    let port: UInt16
    let maximumImageBytes: Int
    let bearerToken: String

    static func makeDefault(defaults: UserDefaults = .standard) -> LocalServerConfiguration {
        LocalServerConfiguration(
            port: 8765,
            maximumImageBytes: 10 * 1024 * 1024,
            bearerToken: loadOrCreateToken(defaults: defaults)
        )
    }

    private static let tokenDefaultsKey = "localInferenceServerBearerToken"

    private static func loadOrCreateToken(defaults: UserDefaults) -> String {
        if let existing = defaults.string(forKey: tokenDefaultsKey), !existing.isEmpty {
            return existing
        }

        let token = UUID().uuidString
        defaults.set(token, forKey: tokenDefaultsKey)
        return token
    }
}
