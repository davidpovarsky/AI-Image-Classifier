import Foundation
import Observation

@MainActor
@Observable
final class CameraViewModel {
    static let shared = CameraViewModel()

    private(set) var prediction = "Detecting..."
    private var isProcessing = false
    private let policy = NudityFilterPolicy()

    private init() {}

    func processFrame(imageData: Data) {
        guard !isProcessing else { return }
        isProcessing = true
        Task { [weak self] in
            guard let self else { return }
            defer { isProcessing = false }
            do {
                let batch = try await NudeNetService.shared.detect(imageData: imageData)
                let decision = policy.evaluate(batch.detections)
                if let top = batch.detections.first {
                    prediction = "\(decision.allowed ? "Allowed" : "Blocked"): \(top.label) (\(Int(top.confidence * 100))%)"
                } else {
                    prediction = "Allowed: no detections"
                }
            } catch {
                prediction = "Detection unavailable"
            }
        }
    }
}
