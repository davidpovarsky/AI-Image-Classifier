import Foundation
import UIKit

@MainActor
final class ImageRecognitionViewModel: ObservableObject {
    @Published var detections: [NudeDetection] = []
    @Published var decision: NudityPolicyDecision?
    @Published var errorMessage: String?
    @Published var isLoading = false

    private let policy = NudityFilterPolicy()

    func classify(image: UIImage) {
        errorMessage = nil
        isLoading = true
        Task { [weak self] in
            guard let self else { return }
            do {
                let batch = try await NudeNetService.shared.detect(image: image)
                detections = batch.detections
                decision = policy.evaluate(batch.detections)
                isLoading = false
            } catch {
                isLoading = false
                detections = []
                decision = nil
                errorMessage = "Nudity detection failed. Try another image."
            }
        }
    }
}
