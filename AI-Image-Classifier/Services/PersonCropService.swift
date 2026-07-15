import CoreGraphics

nonisolated struct PersonCropService: Sendable {
    let paddingFraction: CGFloat

    init(paddingFraction: CGFloat = PersonClassifierConfiguration.default.cropPaddingFraction) {
        self.paddingFraction = paddingFraction
    }

    func crop(image: CGImage, detection: HumanDetection) -> PersonCrop? {
        let visionBox = detection.boundingBox.clampedToUnitSquare
        let padded = CGRect(
            x: visionBox.minX - visionBox.width * paddingFraction,
            y: visionBox.minY - visionBox.height * paddingFraction,
            width: visionBox.width * (1 + 2 * paddingFraction),
            height: visionBox.height * (1 + 2 * paddingFraction)
        ).clampedToUnitSquare
        let pixelRect = Self.pixelRect(forVisionBox: padded, imageWidth: image.width, imageHeight: image.height)
        guard !pixelRect.isEmpty, let cropped = image.cropping(to: pixelRect) else { return nil }
        return PersonCrop(
            id: detection.id,
            image: cropped,
            sourceBoundingBox: Self.topLeftBox(fromVisionBox: visionBox),
            detectionConfidence: detection.confidence,
            detectionSource: detection.source
        )
    }

    static func topLeftBox(fromVisionBox box: CGRect) -> CGRect {
        CGRect(x: box.minX, y: 1 - box.maxY, width: box.width, height: box.height).clampedToUnitSquare
    }

    static func pixelRect(forVisionBox box: CGRect, imageWidth: Int, imageHeight: Int) -> CGRect {
        CGRect(
            x: box.minX * CGFloat(imageWidth),
            y: (1 - box.maxY) * CGFloat(imageHeight),
            width: box.width * CGFloat(imageWidth),
            height: box.height * CGFloat(imageHeight)
        ).integral.intersection(CGRect(x: 0, y: 0, width: imageWidth, height: imageHeight))
    }
}
