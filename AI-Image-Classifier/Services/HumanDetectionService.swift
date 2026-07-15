import CoreGraphics
import ImageIO
import Vision

nonisolated struct HumanDetectionService: Sendable {
    enum DetectionError: Error { case visionFailed }

    func detect(in image: CGImage, orientation: CGImagePropertyOrientation) throws -> [HumanDetection] {
        let request = VNDetectHumanRectanglesRequest()
        request.upperBodyOnly = false
        do {
            try VNImageRequestHandler(cgImage: image, orientation: orientation).perform([request])
        } catch {
            throw DetectionError.visionFailed
        }
        return (request.results ?? []).map {
            HumanDetection(
                id: UUID(),
                confidence: $0.confidence,
                boundingBox: $0.boundingBox,
                source: .humanRectangle
            )
        }
    }

    func detectFaces(in image: CGImage, orientation: CGImagePropertyOrientation) throws -> [HumanDetection] {
        let request = VNDetectFaceRectanglesRequest()
        try VNImageRequestHandler(cgImage: image, orientation: orientation).perform([request])
        return (request.results ?? []).map { observation in
            let face = observation.boundingBox
            let expanded = CGRect(
                x: face.minX - face.width * 0.75,
                y: face.minY - face.height * 3.5,
                width: face.width * 2.5,
                height: face.height * 5
            ).clampedToUnitSquare
            return HumanDetection(
                id: UUID(),
                confidence: observation.confidence,
                boundingBox: expanded,
                source: .faceFallback
            )
        }
    }
}

nonisolated extension CGRect {
    var clampedToUnitSquare: CGRect {
        intersection(CGRect(x: 0, y: 0, width: 1, height: 1))
    }
}
