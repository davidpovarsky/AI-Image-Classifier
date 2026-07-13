import Foundation

nonisolated struct NormalizedBoundingBox: Codable, Equatable, Sendable {
    let x: Double
    let y: Double
    let width: Double
    let height: Double

    init(x: Double, y: Double, width: Double, height: Double) {
        let normalizedX = min(max(x, 0), 1)
        let normalizedY = min(max(y, 0), 1)
        self.x = normalizedX
        self.y = normalizedY
        self.width = min(max(width, 0), 1 - normalizedX)
        self.height = min(max(height, 0), 1 - normalizedY)
    }
}

nonisolated struct NudeDetection: Codable, Equatable, Sendable {
    let classId: Int
    let label: String
    let confidence: Double
    let boundingBox: NormalizedBoundingBox

    enum CodingKeys: String, CodingKey {
        case classId
        case label
        case confidence
        case boundingBox = "box"
    }
}

nonisolated struct NudeDetectionBatch: Equatable, Sendable {
    let detections: [NudeDetection]
    let inferenceDurationMs: Int
}

nonisolated enum NudeNetLabels {
    static let all: [String] = [
        "FEMALE_GENITALIA_COVERED",
        "FACE_FEMALE",
        "BUTTOCKS_EXPOSED",
        "FEMALE_BREAST_EXPOSED",
        "FEMALE_GENITALIA_EXPOSED",
        "MALE_BREAST_EXPOSED",
        "ANUS_EXPOSED",
        "FEET_EXPOSED",
        "BELLY_COVERED",
        "FEET_COVERED",
        "ARMPITS_COVERED",
        "ARMPITS_EXPOSED",
        "FACE_MALE",
        "BELLY_EXPOSED",
        "MALE_GENITALIA_EXPOSED",
        "ANUS_COVERED",
        "FEMALE_BREAST_COVERED",
        "BUTTOCKS_COVERED"
    ]

    static func classId(for label: String) -> Int? {
        all.firstIndex(of: label)
    }
}
