import XCTest
@testable import AI_Image_Classifier

final class PersonClassifierTests: XCTestCase {
    func testVisionCoordinateConversionInvertsYAxisExactlyOnce() {
        let vision = CGRect(x: 0.1, y: 0.2, width: 0.3, height: 0.4)
        let topLeft = PersonCropService.topLeftBox(fromVisionBox: vision)
        XCTAssertEqual(topLeft.minX, 0.1, accuracy: 0.0001)
        XCTAssertEqual(topLeft.minY, 0.4, accuracy: 0.0001)
        XCTAssertEqual(topLeft.width, 0.3, accuracy: 0.0001)
        XCTAssertEqual(topLeft.height, 0.4, accuracy: 0.0001)
    }

    func testVisionBoxToPixelCrop() {
        let pixels = PersonCropService.pixelRect(
            forVisionBox: CGRect(x: 0.25, y: 0.1, width: 0.5, height: 0.6),
            imageWidth: 1_000,
            imageHeight: 500
        )
        XCTAssertEqual(pixels, CGRect(x: 250, y: 150, width: 500, height: 300))
    }

    func testCropExpansionClampsAtImageBoundary() {
        let image = makeCGImage(width: 100, height: 200)
        let detection = HumanDetection(
            id: UUID(), confidence: 0.9,
            boundingBox: CGRect(x: 0, y: 0, width: 0.2, height: 0.2),
            source: .humanRectangle
        )
        let crop = PersonCropService(paddingFraction: 0.12).crop(image: image, detection: detection)
        XCTAssertNotNil(crop)
        XCTAssertGreaterThan(crop?.image.width ?? 0, 20)
        XCTAssertGreaterThan(crop?.image.height ?? 0, 40)
    }

    func testBoundingBoxIsNormalized() {
        let box = NormalizedBoundingBox(x: -0.2, y: 0.8, width: 1.4, height: 0.5)
        XCTAssertEqual(box.x, 0)
        XCTAssertEqual(box.y, 0.8)
        XCTAssertEqual(box.width, 1)
        XCTAssertEqual(box.height, 0.2, accuracy: 0.0001)
    }

    func testPromptConfigurationUsesMultiplePromptsPerCategory() {
        for visualClass in PersonVisualClass.allCases {
            XCTAssertGreaterThan(MobileCLIPPromptConfiguration.prompts[visualClass, default: []].count, 1)
        }
    }

    func testCategoryEmbeddingNormalizesEachPromptThenMean() throws {
        let result = try XCTUnwrap(EmbeddingMath.categoryEmbedding([[3, 0], [0, 4]]))
        XCTAssertEqual(result[0], Float(1 / sqrt(2.0)), accuracy: 0.0001)
        XCTAssertEqual(result[1], Float(1 / sqrt(2.0)), accuracy: 0.0001)
    }

    func testCosineSimilarityAndSoftmax() throws {
        XCTAssertEqual(try XCTUnwrap(EmbeddingMath.cosineSimilarity([1, 0], [1, 0])), 1, accuracy: 0.0001)
        let scores = try XCTUnwrap(EmbeddingMath.softmax([2, 1, 0, -1]))
        XCTAssertEqual(scores.reduce(0, +), 1, accuracy: 0.0001)
        XCTAssertEqual(scores.indices.max(by: { scores[$0] < scores[$1] }), 0)
    }

    func testNumericalProtectionRejectsZeroAndNaN() {
        XCTAssertNil(EmbeddingMath.normalize([0, 0]))
        XCTAssertNil(EmbeddingMath.normalize([.nan, 1]))
        XCTAssertNil(EmbeddingMath.softmax([.infinity, 1]))
    }

    func testPersonAPIResponseEncoding() throws {
        let person = sampleClassification(source: .humanRectangle)
        let response = ClassificationResponseDTO(batch: PersonClassificationBatch(
            imageWidth: 1_920, imageHeight: 1_080, people: [person], inferenceDurationMs: 83
        ))
        let data = try JSONEncoder().encode(response)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(object["model"] as? String, "MobileCLIP2-S2")
        XCTAssertEqual(object["serverVersion"] as? Int, 5)
        XCTAssertEqual(object["peopleCount"] as? Int, 1)
        XCTAssertNil(object["allowed"])
        XCTAssertNil(object["blocked"])
        XCTAssertNil(object["detections"])
    }

    func testAPIResponseSupportsNoPeopleAndFaceFallback() throws {
        let empty = ClassificationResponseDTO(batch: PersonClassificationBatch(
            imageWidth: 20, imageHeight: 10, people: [], inferenceDurationMs: 1
        ))
        XCTAssertEqual(empty.peopleCount, 0)
        XCTAssertEqual(sampleClassification(source: .faceFallback).detectionSource, .faceFallback)
    }

    func testHealthRequiresEveryModelAsset() {
        var metrics = MobileCLIPServiceMetrics()
        metrics.state = .ready
        metrics.imageEncoderLoaded = true
        metrics.promptEmbeddingsReady = true
        metrics.smokeTestPassed = true
        metrics.selectedComputeUnits = "cpuOnly"
        let health = LocalAPIContract.health(from: metrics)
        XCTAssertEqual(health.status, "ok")
        XCTAssertEqual(health.serverVersion, 5)
        XCTAssertEqual(health.model, "MobileCLIP2-S2")
        XCTAssertFalse(health.textEncoderBundled)
        XCTAssertEqual(health.modelPrecision, "float16")
    }

    func testInvalidToken() {
        XCTAssertFalse(LocalAPIContract.isAuthorized(header: nil, token: "secret"))
        XCTAssertFalse(LocalAPIContract.isAuthorized(header: "Bearer wrong", token: "secret"))
        XCTAssertTrue(LocalAPIContract.isAuthorized(header: "Bearer secret", token: "secret"))
    }

    func testDiagnosticRedactionRemovesBearerTokenAndContainerIdentifier() {
        let value = DiagnosticLogService.redact("Bearer secret-token /Bundle/Application/01234567-89AB-CDEF-0123-456789ABCDEF/App.app")
        XCTAssertFalse(value.contains("secret-token"))
        XCTAssertTrue(value.contains("<APP_CONTAINER>"))
    }

    func testErrorSerializationIncludesCoreMLErrorCodeAndUnderlyingError() {
        let underlying = NSError(domain: "com.apple.CoreML", code: -14)
        let outer = NSError(domain: "test", code: 1, userInfo: [NSUnderlyingErrorKey: underlying])
        let errors = DiagnosticLogService.flattenedErrors(outer)
        XCTAssertEqual(errors.map(\.code), [1, -14])
    }

    func testComputeUnitFallbackOrderIsDiagnosticFirst() {
        XCTAssertEqual(MobileCLIPService.computeUnitFallbackOrder, ["cpuAndGPU", "cpuOnly", "all"])
    }

    func testDiagnosticSessionCreatesFilesWritesJSONLAndExports() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let service = DiagnosticLogService(documentsURL: root)
        try await service.startSession()
        await withTaskGroup(of: Void.self) { group in
            for index in 0..<20 {
                group.addTask { try? await service.log(level: "info", category: "test", event: "event-\(index)") }
            }
        }
        let optionalSnapshot = await service.snapshot()
        let snapshot = try XCTUnwrap(optionalSnapshot)
        XCTAssertEqual(snapshot.eventCount, 21)
        for name in ["runtime.log", "events.jsonl", "device.json", "app.json", "model-manifest.json", "model-load-attempts.json", "health-snapshots.jsonl", "inference-events.jsonl", "summary.json"] {
            XCTAssertTrue(FileManager.default.fileExists(atPath: snapshot.folderURL.appending(path: name).path()))
        }
        let lines = try String(contentsOf: snapshot.folderURL.appending(path: "events.jsonl"), encoding: .utf8)
            .split(separator: "\n")
        XCTAssertEqual(lines.count, 21)
        let export = try await service.exportLatestSession()
        XCTAssertTrue(FileManager.default.fileExists(atPath: export.path()))
    }

    func testUIImageOrientationMapping() {
        XCTAssertEqual(UIImage.Orientation.up.cgImagePropertyOrientation, .up)
        XCTAssertEqual(UIImage.Orientation.downMirrored.cgImagePropertyOrientation, .downMirrored)
        XCTAssertEqual(UIImage.Orientation.left.cgImagePropertyOrientation, .left)
        XCTAssertEqual(UIImage.Orientation.rightMirrored.cgImagePropertyOrientation, .rightMirrored)
    }

    private func sampleClassification(source: DetectionSource) -> PersonClassification {
        PersonClassification(
            id: UUID(), detectionSource: source, personDetectionConfidence: 0.94,
            boundingBox: NormalizedBoundingBox(x: 0.12, y: 0.08, width: 0.31, height: 0.82),
            predictedClass: .woman, confidence: 0.81,
            scores: PersonScores(woman: 0.81, man: 0.10, uncertain: 0.07, notPerson: 0.02)
        )
    }

    private func makeCGImage(width: Int, height: Int) -> CGImage {
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: width * 4, space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )!
        return context.makeImage()!
    }
}
