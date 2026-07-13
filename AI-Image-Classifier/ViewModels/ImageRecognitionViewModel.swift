//
//  ImageRecognitionViewModel.swift
//  ImageRecognitionMealScanning
//
//  Created by Goutam Roy on 13/04/26.
//

import Foundation
import UIKit
import Combine

final class ImageRecognitionViewModel: ObservableObject {
    
    @Published var results: [PredictionResult] = []
    @Published var errorMessage: String?
    @Published var isLoading = false
    
    func classify(image: UIImage) {
        errorMessage = nil
        isLoading = true

        Task { [weak self] in
            guard let self else { return }

            do {
                let predictions = try await CoreMLImageClassifier.shared.classify(image: image)
                self.isLoading = false

                if predictions.isEmpty {
                    self.errorMessage = "No objects recognized. Try a clearer image."
                    self.results = []
                    return
                }

                self.results = predictions
                    .map {
                        PredictionResult(identifier: $0.label,
                                         confidence: Double($0.confidence))
                    }
                    .sorted { $0.confidence > $1.confidence }
            } catch {
                self.isLoading = false
                self.results = []
                self.errorMessage = "Image classification failed. Try another image."
            }
        }
    }
}
