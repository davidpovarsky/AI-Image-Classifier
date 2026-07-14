import XCTest
@testable import AI_Image_Classifier

final class NudeNetTests: XCTestCase {
    func testCanonicalLabelMapping() {
        XCTAssertEqual(NudeNetLabels.all.count, 18)
        XCTAssertEqual(Set(NudeNetLabels.all).count, 18)
        XCTAssertEqual(NudeNetLabels.classId(for: "FEMALE_GENITALIA_COVERED"), 0)
        XCTAssertEqual(NudeNetLabels.classId(for: "FEMALE_BREAST_EXPOSED"), 3)
        XCTAssertEqual(NudeNetLabels.classId(for: "BUTTOCKS_COVERED"), 17)
    }

    func testStandardBlockingPolicy() {
        let policy = NudityFilterPolicy()
        XCTAssertFalse(policy.evaluate([detection("FEMALE_BREAST_EXPOSED", 0.46)]).allowed)
        XCTAssertFalse(policy.evaluate([detection("FEMALE_GENITALIA_EXPOSED", 0.36)]).allowed)
        XCTAssertTrue(policy.evaluate([detection("FEMALE_BREAST_COVERED", 0.99)]).allowed)
        XCTAssertTrue(policy.evaluate([detection("FACE_FEMALE", 0.99)]).allowed)
        XCTAssertTrue(policy.evaluate([detection("FEMALE_BREAST_EXPOSED", 0.44)]).allowed)
    }

    func testOptionalClassesOnlyBlockInStrictMode() {
        let sample = [detection("MALE_BREAST_EXPOSED", 0.80)]
        XCTAssertTrue(NudityFilterPolicy().evaluate(sample).allowed)
        XCTAssertFalse(NudityFilterPolicy(mode: .strict).evaluate(sample).allowed)
    }

    func testRawInferenceAPIResponseEncoding() throws {
        let batch = NudeDetectionBatch(
            detections: [
                detection("FACE_FEMALE", 0.9),
                detection("FEMALE_BREAST_COVERED", 0.7)
            ],
            inferenceDurationMs: 12
        )

        let response = ClassificationResponseDTO(batch: batch)
        let data = try JSONEncoder().encode(response)
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: data) as? [String: Any]
        )

        XCTAssertEqual(object["success"] as? Bool, true)
        XCTAssertEqual(object["model"] as? String, "NudeNet320n")
        XCTAssertEqual(object["durationMs"] as? Int, 12)
        XCTAssertEqual((object["detections"] as? [[String: Any]])?.count, 2)
        XCTAssertEqual(Set(object.keys), ["success", "model", "durationMs", "detections"])
        XCTAssertNil(object["allowed"])
        XCTAssertNil(object["risk"])
        XCTAssertNil(object["confidence"])
        XCTAssertNil(object["triggeredClass"])
        XCTAssertNil(object["predictions"])
        XCTAssertNil(object["policyVersion"])
    }

    func testInvalidImageInput() async {
        do {
            _ = try await NudeNetService().detect(imageData: Data("not-image".utf8))
            XCTFail("Expected invalid image")
        } catch {
            XCTAssertEqual(error as? NudeNetService.ServiceError, .invalidImage)
        }
    }

    func testInvalidToken() {
        XCTAssertFalse(LocalAPIContract.isAuthorized(header: nil, token: "secret"))
        XCTAssertFalse(LocalAPIContract.isAuthorized(header: "Bearer wrong", token: "secret"))
        XCTAssertTrue(LocalAPIContract.isAuthorized(header: "Bearer secret", token: "secret"))
    }

    func testModelLoadFailureIsReported() async {
        let service = NudeNetService { throw NudeNetService.ServiceError.modelUnavailable }
        do {
            try await service.loadIfNeeded()
            XCTFail("Expected model failure")
        } catch {
            let snapshot = await service.snapshot()
            if case .failed = snapshot.state { } else { XCTFail("Expected failed state") }
        }
    }

    func testRepeatedLoadingRetainsOneModel() async throws {
        let service = NudeNetService()
        try await service.loadIfNeeded()
        try await service.loadIfNeeded()
        let count = await service.snapshot().modelLoadCount
        XCTAssertEqual(count, 1)
    }

    func testBoundingBoxIsNormalized() {
        let box = NormalizedBoundingBox(x: -0.2, y: 0.8, width: 1.4, height: 0.5)
        XCTAssertEqual(box.x, 0)
        XCTAssertEqual(box.y, 0.8)
        XCTAssertEqual(box.width, 1)
        XCTAssertEqual(box.height, 0.2, accuracy: 0.0001)
    }

    func testUIImageOrientationMapping() {
        XCTAssertEqual(UIImage.Orientation.up.cgImagePropertyOrientation, .up)
        XCTAssertEqual(UIImage.Orientation.downMirrored.cgImagePropertyOrientation, .downMirrored)
        XCTAssertEqual(UIImage.Orientation.left.cgImagePropertyOrientation, .left)
        XCTAssertEqual(UIImage.Orientation.rightMirrored.cgImagePropertyOrientation, .rightMirrored)
    }

    func testHealthReportsReadinessAndModel() {
        var loading = NudeNetServiceMetrics()
        XCTAssertEqual(LocalAPIContract.health(from: loading).status, "unavailable")
        loading.state = .ready
        let ready = LocalAPIContract.health(from: loading)
        XCTAssertEqual(ready.status, "ok")
        XCTAssertEqual(ready.model, "NudeNet320n")
        XCTAssertTrue(ready.modelLoaded)
        XCTAssertEqual(ready.serverVersion, 3)
    }

    func testLiveServerHealthAndSafeJPEGClassification() async throws {
        let configuration = LocalServerConfiguration(
            port: 8765,
            maximumImageBytes: 10 * 1024 * 1024,
            bearerToken: "unit-test-token"
        )
        let server = LocalInferenceServer(configuration: configuration)
        server.start()
        defer { server.stop() }

        var healthData: Data?
        for _ in 0..<60 {
            if let (data, response) = try? await URLSession.shared.data(from: server.localURL.appending(path: "health")),
               (response as? HTTPURLResponse)?.statusCode == 200 {
                healthData = data
                break
            }
            try await Task.sleep(for: .milliseconds(250))
        }
        let health = try JSONDecoder().decode(HealthResponseDTO.self, from: XCTUnwrap(healthData))
        XCTAssertEqual(health.model, "NudeNet320n")
        XCTAssertTrue(health.modelLoaded)

        let renderer = UIGraphicsImageRenderer(size: CGSize(width: 32, height: 32))
        let image = renderer.image { context in
            UIColor.systemBlue.setFill()
            context.cgContext.fill(CGRect(x: 0, y: 0, width: 32, height: 32))
        }
        var request = URLRequest(url: server.localURL.appending(path: "v1/classify"))
        request.httpMethod = "POST"
        request.setValue("Bearer unit-test-token", forHTTPHeaderField: "Authorization")
        request.setValue("image/jpeg", forHTTPHeaderField: "Content-Type")
        request.httpBody = try XCTUnwrap(image.jpegData(compressionQuality: 0.8))
        let (data, response) = try await URLSession.shared.data(for: request)
        XCTAssertEqual((response as? HTTPURLResponse)?.statusCode, 200)
        let classification = try JSONDecoder().decode(ClassificationResponseDTO.self, from: data)
        XCTAssertTrue(classification.success)
        XCTAssertEqual(classification.model, "NudeNet320n")
        XCTAssertGreaterThanOrEqual(classification.durationMs, 0)
    }

    private func detection(_ label: String, _ confidence: Double) -> NudeDetection {
        NudeDetection(
            classId: NudeNetLabels.classId(for: label)!,
            label: label,
            confidence: confidence,
            boundingBox: NormalizedBoundingBox(x: 0.1, y: 0.2, width: 0.3, height: 0.4)
        )
    }
}
