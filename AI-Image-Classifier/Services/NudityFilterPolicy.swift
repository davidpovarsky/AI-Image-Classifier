import Foundation

nonisolated struct NudityFilterThresholds: Codable, Equatable, Sendable {
    var femaleBreastExposed: Double = 0.45
    var femaleGenitaliaExposed: Double = 0.35
    var maleGenitaliaExposed: Double = 0.35
    var anusExposed: Double = 0.35
    var buttocksExposed: Double = 0.50
    var maleBreastExposed: Double = 0.75
    var bellyExposed: Double = 0.90
    var armpitsExposed: Double = 0.95
}

nonisolated enum NudityFilterMode: String, Codable, Sendable {
    case standard
    case strict
}

nonisolated struct NudityPolicyDecision: Equatable, Sendable {
    let allowed: Bool
    let risk: String
    let confidence: Double
    let triggeredClass: String?
    let detections: [NudeDetection]
}

nonisolated struct NudityFilterPolicy: Sendable {
    static let version = 1

    var thresholds = NudityFilterThresholds()
    var mode: NudityFilterMode = .standard

    var profileName: String { mode.rawValue }

    func evaluate(_ detections: [NudeDetection]) -> NudityPolicyDecision {
        let triggering = detections
            .filter { detection in
                guard let threshold = threshold(for: detection.label) else { return false }
                return detection.confidence >= threshold
            }
            .max { $0.confidence < $1.confidence }

        return NudityPolicyDecision(
            allowed: triggering == nil,
            risk: triggering == nil ? "none" : "nudity",
            confidence: triggering?.confidence ?? 0,
            triggeredClass: triggering?.label,
            detections: detections
        )
    }

    private func threshold(for label: String) -> Double? {
        switch label {
        case "FEMALE_BREAST_EXPOSED": thresholds.femaleBreastExposed
        case "FEMALE_GENITALIA_EXPOSED": thresholds.femaleGenitaliaExposed
        case "MALE_GENITALIA_EXPOSED": thresholds.maleGenitaliaExposed
        case "ANUS_EXPOSED": thresholds.anusExposed
        case "BUTTOCKS_EXPOSED": thresholds.buttocksExposed
        case "MALE_BREAST_EXPOSED" where mode == .strict: thresholds.maleBreastExposed
        case "BELLY_EXPOSED" where mode == .strict: thresholds.bellyExposed
        case "ARMPITS_EXPOSED" where mode == .strict: thresholds.armpitsExposed
        default: nil
        }
    }
}
