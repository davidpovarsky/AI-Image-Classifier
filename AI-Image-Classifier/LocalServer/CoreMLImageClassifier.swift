import Foundation
import UIKit

actor CoreMLImageClassifier {
    static let shared = CoreMLImageClassifier()

    private let service = CoreMLService.shared

    func classify(imageData: Data) async throws -> [ClassificationPrediction] {
        try await service.classify(imageData: imageData)
    }

    func classify(image: UIImage) async throws -> [ClassificationPrediction] {
        try await service.classify(image: image)
    }
}
