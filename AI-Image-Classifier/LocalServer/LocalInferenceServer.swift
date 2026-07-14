import FlyingFox
import FlyingSocks
import Foundation
import Observation
import OSLog

nonisolated struct LocalInferenceMetricsSnapshot: Equatable, Sendable {
    var model = NudeNetServiceMetrics()
    var totalProcessed = 0
    var totalDetections = 0
    var lastDetectionCount = 0
    var lastTopDetection: String?
}

actor LocalInferenceMetrics {
    private var value = LocalInferenceMetricsSnapshot()

    func updateModel(_ model: NudeNetServiceMetrics) { value.model = model }

    func record(detections: [NudeDetection], inferenceDurationMs: Int) {
        value.totalProcessed += 1
        value.totalDetections += detections.count
        value.lastDetectionCount = detections.count
        value.lastTopDetection = detections.first?.label
        value.model.lastInferenceDurationMs = inferenceDurationMs
    }

    func snapshot() -> LocalInferenceMetricsSnapshot { value }
}

@MainActor
@Observable
final class LocalInferenceServer {
    enum State: Equatable {
        case stopped
        case starting
        case running
        case failed
    }

    private static let logger = Logger(
        subsystem: Bundle.main.bundleIdentifier ?? "AI-Image-Classifier",
        category: "server"
    )

    private(set) var state: State = .stopped
    private(set) var lastError: String?
    private(set) var metricsSnapshot = LocalInferenceMetricsSnapshot()

    let configuration: LocalServerConfiguration
    let modelName = LocalServerConfiguration.modelName

    private let server: HTTPServer
    private let detector: NudeNetService
    private let metrics: LocalInferenceMetrics
    private var lifecycleTask: Task<Void, Never>?
    private var modelPreparationTask: Task<Void, Never>?
    private var lifecycleID: UUID?
    private var routesConfigured = false

    init(
        configuration: LocalServerConfiguration = .makeDefault(),
        detector: NudeNetService = .shared
    ) {
        self.configuration = configuration
        self.detector = detector
        metrics = LocalInferenceMetrics()
        let address: sockaddr_in
        do {
            address = try sockaddr_in.inet(ip4: LocalServerConfiguration.host, port: configuration.port)
        } catch {
            preconditionFailure("The built-in loopback address is invalid.")
        }
        server = HTTPServer(address: address)
    }

    var stopped: Bool { state == .stopped }
    var starting: Bool { state == .starting }
    var running: Bool { state == .running }
    var failed: Bool { state == .failed }
    var isRunning: Bool { running }
    var token: String { configuration.bearerToken }
    var localURL: URL {
        var components = URLComponents()
        components.scheme = "http"
        components.host = LocalServerConfiguration.host
        components.port = Int(configuration.port)
        guard let url = components.url else { preconditionFailure("Invalid local server URL.") }
        return url
    }

    func start() {
        guard lifecycleTask == nil, !running, !starting else { return }
        state = .starting
        lastError = nil
        let id = UUID()
        lifecycleID = id
        lifecycleTask = Task { [weak self] in await self?.runServer(lifecycleID: id) }
    }

    func stop() {
        modelPreparationTask?.cancel()
        modelPreparationTask = nil
        guard lifecycleTask != nil || running || starting else { state = .stopped; return }
        lifecycleTask?.cancel()
        let id = UUID()
        lifecycleID = id
        state = .stopped
        lifecycleTask = Task { [weak self] in
            guard let self else { return }
            await server.stop()
            if lifecycleID == id { lifecycleID = nil; lifecycleTask = nil }
        }
    }

    func restart() {
        modelPreparationTask?.cancel()
        modelPreparationTask = nil
        lifecycleTask?.cancel()
        let id = UUID()
        lifecycleID = id
        state = .starting
        lastError = nil
        lifecycleTask = Task { [weak self] in
            guard let self else { return }
            await server.stop()
            if lifecycleID == id {
                lifecycleID = nil
                lifecycleTask = nil
                start()
            }
        }
    }

    func refreshMetrics() async {
        let modelSnapshot = await detector.snapshot()
        await metrics.updateModel(modelSnapshot)
        metricsSnapshot = await metrics.snapshot()
        if case .failed(let message) = modelSnapshot.state { lastError = message }
    }

    private func runServer(lifecycleID: UUID) async {
        await configureRoutesIfNeeded()
        let runTask = Task { try await server.run() }
        do {
            try await server.waitUntilListening()
            try Task.checkCancellation()
            guard self.lifecycleID == lifecycleID else { runTask.cancel(); return }
            state = .running
            Self.logger.info("Local inference server is listening")
            modelPreparationTask = Task { [weak self] in await self?.prepareModel() }
            try await runTask.value
            if !Task.isCancelled, self.lifecycleID == lifecycleID { state = .stopped }
        } catch is CancellationError {
            runTask.cancel()
            await server.stop()
            if self.lifecycleID == lifecycleID { state = .stopped }
        } catch {
            runTask.cancel()
            await server.stop()
            if self.lifecycleID == lifecycleID {
                lastError = "The local server could not start or remain active."
                state = .failed
            }
        }
        if self.lifecycleID == lifecycleID { self.lifecycleID = nil; lifecycleTask = nil }
    }

    private func prepareModel() async {
        do {
            try await detector.loadIfNeeded()
            await refreshMetrics()
            try await detector.warmUp()
            await refreshMetrics()
        } catch {
            await refreshMetrics()
        }
    }

    private func configureRoutesIfNeeded() async {
        guard !routesConfigured else { return }
        routesConfigured = true
        let detector = detector
        await server.appendRoute("GET /health") { _ in
            let health = LocalAPIContract.health(from: await detector.snapshot())
            return JSONHTTPResponse.make(health, statusCode: health.modelLoaded ? .ok : .serviceUnavailable)
        }

        let configuration = configuration
        let metrics = metrics
        await server.appendRoute("POST /v1/classify") { request in
            guard LocalAPIContract.isAuthorized(
                header: request.headers[.authorization],
                token: configuration.bearerToken
            ) else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unauthorized"),
                    statusCode: .unauthorized
                )
            }
            let contentType = request.headers[.contentType]?
                .split(separator: ";", maxSplits: 1).first?
                .trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            let supported: Set<String> = ["image/jpeg", "image/png", "image/webp", "image/heic", "image/heif"]
            guard let contentType, supported.contains(contentType) else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unsupported_media_type"),
                    statusCode: .unsupportedMediaType
                )
            }
            if let length = request.headers[.contentLength],
               let count = Int(length), count > configuration.maximumImageBytes {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "payload_too_large"),
                    statusCode: .payloadTooLarge
                )
            }
            let imageData: Data
            do { imageData = try await request.bodyData } catch {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "invalid_image"),
                    statusCode: .badRequest
                )
            }
            guard imageData.count <= configuration.maximumImageBytes else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "payload_too_large"),
                    statusCode: .payloadTooLarge
                )
            }
            guard (await detector.snapshot()).state == .ready else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "model_unavailable"),
                    statusCode: .serviceUnavailable
                )
            }
            let queued = ContinuousClock.now
            do {
                let batch = try await detector.detect(imageData: imageData)
                let waitMs = Self.milliseconds(since: queued) - batch.inferenceDurationMs
                Logger(subsystem: Bundle.main.bundleIdentifier ?? "AI-Image-Classifier", category: "inference")
                    .debug("Queue wait \(max(waitMs, 0), privacy: .public) ms; inference \(batch.inferenceDurationMs, privacy: .public) ms")
                await metrics.record(
                    detections: batch.detections,
                    inferenceDurationMs: batch.inferenceDurationMs
                )
                return JSONHTTPResponse.make(ClassificationResponseDTO(batch: batch))
            } catch NudeNetService.ServiceError.invalidImage {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "invalid_image"),
                    statusCode: .badRequest
                )
            } catch NudeNetService.ServiceError.modelUnavailable {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "model_unavailable"),
                    statusCode: .serviceUnavailable
                )
            } catch {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "classification_failed"),
                    statusCode: .internalServerError
                )
            }
        }
    }

    nonisolated private static func milliseconds(since start: ContinuousClock.Instant) -> Int {
        let duration = start.duration(to: .now)
        return Int(duration.components.seconds * 1_000)
            + Int(duration.components.attoseconds / 1_000_000_000_000_000)
    }
}
