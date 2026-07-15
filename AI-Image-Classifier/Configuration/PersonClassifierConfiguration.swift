import Foundation

nonisolated struct PersonClassifierConfiguration: Sendable {
    let cropPaddingFraction: CGFloat
    let logitScale: Float
    let useFaceFallback: Bool
    let useWholeImageFallback: Bool

    static let `default` = PersonClassifierConfiguration(
        cropPaddingFraction: 0.12,
        logitScale: 100,
        useFaceFallback: true,
        useWholeImageFallback: false
    )
}
