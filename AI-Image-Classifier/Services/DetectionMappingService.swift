import CoreGraphics

nonisolated struct DetectionMappingService: Sendable {
    static let responseCoordinateSystem = "normalized-top-left"

    func topLeft(fromVisionBottomLeft box: NormalizedBoundingBox) -> NormalizedBoundingBox {
        NormalizedBoundingBox(x: box.x, y: 1 - box.y - box.height, width: box.width, height: box.height)
    }

    func originalImageBox(
        cropTopLeft: NormalizedBoundingBox,
        detectionInCropBottomLeft: NormalizedBoundingBox
    ) -> NormalizedBoundingBox {
        let local = topLeft(fromVisionBottomLeft: detectionInCropBottomLeft)
        return NormalizedBoundingBox(
            x: cropTopLeft.x + local.x * cropTopLeft.width,
            y: cropTopLeft.y + local.y * cropTopLeft.height,
            width: local.width * cropTopLeft.width,
            height: local.height * cropTopLeft.height
        )
    }

    func normalized(pixelRect: PixelRectangle, imageWidth: Int, imageHeight: Int) -> NormalizedBoundingBox {
        guard imageWidth > 0, imageHeight > 0 else {
            return NormalizedBoundingBox(x: 0, y: 0, width: 0, height: 0)
        }
        return NormalizedBoundingBox(
            x: Double(pixelRect.x) / Double(imageWidth),
            y: Double(pixelRect.y) / Double(imageHeight),
            width: Double(pixelRect.width) / Double(imageWidth),
            height: Double(pixelRect.height) / Double(imageHeight)
        )
    }
}
