//
//  CoreMLService.swift
//  ImageRecognitionMealScanning
//
//  Created by Goutam Roy on 13/04/26.
//

//import CoreML
//import Vision
//import UIKit
//
//final class CoreMLService {
//    
//    private let model: VNCoreMLModel
//    
//    init() {
//        do {
//            let config = MLModelConfiguration()
//            let coreMLModel = try FoodClassifier(configuration: config).model
//            self.model = try VNCoreMLModel(for: coreMLModel)
//        } catch {
//            fatalError("❌ Failed to load CoreML model: \(error)")
//        }
//    }
//    
//    func classify(image: UIImage, completion: @escaping ([VNClassificationObservation]) -> Void) {
//        
//        guard let ciImage = CIImage(image: image) else {
//            completion([])
//            return
//        }
//        
//        let request = VNCoreMLRequest(model: model) { request, error in
//            
//            if let error = error {
//                print("❌ Vision Error:", error.localizedDescription)
//                completion([])
//                return
//            }
//            
//            let results = request.results as? [VNClassificationObservation] ?? []
//            completion(results)
//        }
//        
//        request.imageCropAndScaleOption = .centerCrop
//        
//        let handler = VNImageRequestHandler(ciImage: ciImage, options: [:])
//        
//        DispatchQueue.global(qos: .userInitiated).async {
//            do {
//                try handler.perform([request])
//            } catch {
//                print("❌ CoreML Error:", error.localizedDescription)
//                completion([])
//            }
//        }
//    }
//}

import CoreML
import UIKit
import Vision

actor CoreMLService {
    static let shared = CoreMLService()

    enum ServiceError: Error {
        case invalidImage
        case modelUnavailable
        case classificationFailed
    }

    private let model: Result<VNCoreMLModel, Error>

    private init() {
        do {
            let config = MLModelConfiguration()
            config.computeUnits = .all
            let coreMLModel = try MobileNetV2(configuration: config).model
            model = .success(try VNCoreMLModel(for: coreMLModel))
        } catch {
            model = .failure(error)
        }
    }

    func classify(imageData: Data) throws -> [ClassificationPrediction] {
        guard let image = UIImage(data: imageData) else {
            throw ServiceError.invalidImage
        }
        return try classify(image: image)
    }

    func classify(image: UIImage) throws -> [ClassificationPrediction] {
        guard let ciImage = CIImage(image: image) else {
            throw ServiceError.invalidImage
        }

        let visionModel: VNCoreMLModel
        do {
            visionModel = try model.get()
        } catch {
            throw ServiceError.modelUnavailable
        }

        let request = VNCoreMLRequest(model: visionModel)
        request.imageCropAndScaleOption = .centerCrop

        do {
            try VNImageRequestHandler(ciImage: ciImage).perform([request])
        } catch {
            throw ServiceError.classificationFailed
        }

        guard let observations = request.results as? [VNClassificationObservation] else {
            throw ServiceError.classificationFailed
        }

        return observations
            .map {
                ClassificationPrediction(
                    label: $0.identifier,
                    confidence: Double($0.confidence)
                )
            }
            .sorted { $0.confidence > $1.confidence }
            .prefix(3)
            .map { $0 }
    }
}
