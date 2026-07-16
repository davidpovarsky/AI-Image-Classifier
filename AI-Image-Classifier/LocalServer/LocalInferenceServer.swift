import FlyingFox
import FlyingSocks
import Foundation
import Observation
import OSLog

nonisolated struct LocalInferenceMetricsSnapshot: Equatable, Sendable {
    var model = MobileCLIPServiceMetrics()
    var totalProcessed = 0
    var totalDetectedPeople = 0
    var totalLatencyMs = 0
    var lastRequestLatencyMs: Int?
    var lastResultSummary: String?

    var averageLatencyMs: Int? {
        totalProcessed == 0 ? nil : totalLatencyMs / totalProcessed
    }
}

actor LocalInferenceMetrics {
    private var value = LocalInferenceMetricsSnapshot()

    func updateModel(_ model: MobileCLIPServiceMetrics) { value.model = model }

    func record(batch: PersonClassificationBatch) {
        value.totalProcessed += 1
        value.totalDetectedPeople += batch.people.count
        value.totalLatencyMs += batch.inferenceDurationMs
        value.lastRequestLatencyMs = batch.inferenceDurationMs
        let classes = Dictionary(grouping: batch.people, by: \.predictedClass)
            .map { "\($0.key.rawValue): \($0.value.count)" }
            .sorted()
            .joined(separator: ", ")
        value.lastResultSummary = classes.isEmpty ? "No people detected" : classes
    }

    func snapshot() -> LocalInferenceMetricsSnapshot { value }
}

@MainActor
@Observable
final class LocalInferenceServer {
    enum State: Equatable { case stopped, starting, running, failed }

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
    private let coordinator: PersonInferenceCoordinator
    private let imageSafetyPipeline = ImageSafetyPipelineService.shared
    private let metrics = LocalInferenceMetrics()
    private var lifecycleTask: Task<Void, Never>?
    private var modelPreparationTask: Task<Void, Never>?
    private var lifecycleID: UUID?
    private var routesConfigured = false

    init(
        configuration: LocalServerConfiguration = .makeDefault(),
        coordinator: PersonInferenceCoordinator = .shared
    ) {
        self.configuration = configuration
        self.coordinator = coordinator
        do {
            server = HTTPServer(address: try sockaddr_in.inet(
                ip4: LocalServerConfiguration.host,
                port: configuration.port
            ))
        } catch {
            preconditionFailure("The built-in loopback address is invalid.")
        }
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
        let model = await coordinator.modelSnapshot()
        await metrics.updateModel(model)
        metricsSnapshot = await metrics.snapshot()
        if case .failed(let message) = model.state { lastError = message }
    }

    func retryModelLoad() {
        modelPreparationTask?.cancel()
        modelPreparationTask = Task { [weak self] in
            guard let self else { return }
            do { try await coordinator.reloadModel() } catch { }
            await refreshMetrics()
        }
    }

    private func runServer(lifecycleID: UUID) async {
        await configureRoutesIfNeeded()
        let runTask = Task { try await server.run() }
        do {
            try await server.waitUntilListening()
            try Task.checkCancellation()
            guard self.lifecycleID == lifecycleID else { runTask.cancel(); return }
            state = .running
            Self.logger.info("Local person-classification server is listening")
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
        await imageSafetyPipeline.prepare()
        await refreshMetrics()
    }

    private func configureRoutesIfNeeded() async {
        guard !routesConfigured else { return }
        routesConfigured = true
        let coordinator = coordinator
        let imageSafetyPipeline = imageSafetyPipeline
        await server.appendRoute("GET /health") { (_: HTTPRequest) in
            let readiness = await imageSafetyPipeline.readiness()
            let health = LocalAPIContract.health(from: readiness.mobileCLIP, nudeNet: readiness.nudeNet)
            try? await DiagnosticLogService.shared.appendHealth(health)
            return JSONHTTPResponse.make(health, statusCode: health.modelLoaded ? .ok : .serviceUnavailable)
        }

        let configuration = configuration
        await server.appendRoute("GET /v1/diagnostics/status") { (request: HTTPRequest) in
            guard LocalAPIContract.isAuthorized(
                header: request.headers[.authorization], token: configuration.bearerToken
            ) else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unauthorized"), statusCode: .unauthorized
                )
            }
            let model = await coordinator.modelSnapshot()
            let session = await DiagnosticLogService.shared.snapshot()
            let state: String
            switch model.state {
            case .notLoaded: state = "notLoaded"
            case .loading, .warming: state = "loading"
            case .ready: state = "ready"
            case .failed: state = "failed"
            }
            let safety = await imageSafetyPipeline.snapshot()
            let moduleStatuses = Dictionary(uniqueKeysWithValues: safety.moduleDurations.map {
                ($0.key, ImageSafetyModuleStatusDTO(status: safety.lastStatus == nil ? "ready" : "completed", lastDurationMs: $0.value))
            })
            let response = DiagnosticsStatusDTO(
                sessionId: session?.sessionID ?? "not-started", state: state, stage: model.stage,
                model: LocalServerConfiguration.modelName, precision: "float16", deploymentTarget: "iOS26.5",
                selectedComputeUnits: model.selectedComputeUnits, attempts: model.loadAttempts,
                logFilesAvailable: session != nil,
                imageSafetyPipeline: ImageSafetyDiagnosticsStatusDTO(
                    available: true, pipelineVersion: ImageSafetyPipelineService.pipelineVersion,
                    lastRequestId: safety.lastRequestId, lastStatus: safety.lastStatus,
                    lastTotalDurationMs: safety.lastTotalDurationMs, modules: moduleStatuses
                )
            )
            return JSONHTTPResponse.make(response)
        }

        let metrics = metrics
        await server.appendRoute("POST /v1/image-safety-classify") { (request: HTTPRequest) in
            let requestID = UUID()
            guard LocalAPIContract.isAuthorized(
                header: request.headers[.authorization], token: configuration.bearerToken
            ) else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unauthorized"), statusCode: .unauthorized
                )
            }
            let contentType = request.headers[.contentType]?
                .split(separator: ";", maxSplits: 1).first?
                .trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            let supported: Set<String> = ["image/jpeg", "image/png", "image/heic", "image/heif"]
            guard let contentType, supported.contains(contentType) else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unsupported_media_type"),
                    statusCode: .unsupportedMediaType
                )
            }
            if let length = request.headers[.contentLength],
               let count = Int(length), count > configuration.maximumImageBytes {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "payload_too_large"), statusCode: .payloadTooLarge
                )
            }
            let imageData: Data
            do { imageData = try await request.bodyData } catch {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "invalid_image"), statusCode: .badRequest
                )
            }
            guard imageData.count <= configuration.maximumImageBytes else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "payload_too_large"), statusCode: .payloadTooLarge
                )
            }
            do {
                let response = try await imageSafetyPipeline.classify(
                    imageData: imageData, mimeType: contentType, requestID: requestID
                )
                try? await DiagnosticLogService.shared.log(
                    level: response.pipeline.partialFailure ? "warning" : "info",
                    category: "imageSafety", event: "imageSafetyRequestCompleted",
                    details: [
                        "requestId": response.requestId,
                        "inputByteCount": String(response.input.byteCount),
                        "pixelWidth": String(response.input.pixelWidth),
                        "pixelHeight": String(response.input.pixelHeight),
                        "totalDurationMs": String(response.pipeline.totalDurationMs),
                        "personCount": String(response.summary.personCount),
                        "rawNudityCount": String(response.nudity.allRawDetections.count),
                        "mergedNudityCount": String(response.nudity.mergedDetections.count)
                    ]
                )
                return JSONHTTPResponse.make(response)
            } catch ImageSafetyPipelineService.ServiceError.invalidImage {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "invalid_image"), statusCode: .badRequest
                )
            } catch {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "pipeline_failed"), statusCode: .internalServerError
                )
            }
        }

        await server.appendRoute("POST /v1/person-classify") { (request: HTTPRequest) in
            let requestID = UUID()
            let requestStarted = ContinuousClock.now
            guard LocalAPIContract.isAuthorized(
                header: request.headers[.authorization], token: configuration.bearerToken
            ) else {
                try? await DiagnosticLogService.shared.log(level: "warning", category: "server", event: "requestRejected", details: [
                    "requestId": requestID.uuidString, "method": "POST", "path": "/v1/person-classify", "authenticated": "false", "statusCode": "401"
                ])
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unauthorized"), statusCode: .unauthorized
                )
            }
            let contentType = request.headers[.contentType]?
                .split(separator: ";", maxSplits: 1).first?
                .trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            let supported: Set<String> = ["image/jpeg", "image/png", "image/heic", "image/heif"]
            guard let contentType, supported.contains(contentType) else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unsupported_media_type"),
                    statusCode: .unsupportedMediaType
                )
            }
            if let length = request.headers[.contentLength],
               let count = Int(length), count > configuration.maximumImageBytes {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "payload_too_large"), statusCode: .payloadTooLarge
                )
            }
            let imageData: Data
            do { imageData = try await request.bodyData } catch {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "invalid_image"), statusCode: .badRequest
                )
            }
            guard imageData.count <= configuration.maximumImageBytes else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "payload_too_large"), statusCode: .payloadTooLarge
                )
            }
            guard (await coordinator.modelSnapshot()).state == .ready else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "model_unavailable"),
                    statusCode: .serviceUnavailable
                )
            }
            do {
                let batch = try await coordinator.classify(imageData: imageData, requestID: requestID)
                await metrics.record(batch: batch)
                let elapsed = requestStarted.duration(to: .now)
                let duration = Int(elapsed.components.seconds * 1_000) + Int(elapsed.components.attoseconds / 1_000_000_000_000_000)
                try? await DiagnosticLogService.shared.log(level: "info", category: "server", event: "requestCompleted", details: [
                    "requestId": requestID.uuidString, "method": "POST", "path": "/v1/person-classify",
                    "contentType": contentType, "bodySizeBytes": String(imageData.count), "authenticated": "true",
                    "durationMs": String(duration), "statusCode": "200", "peopleDetected": String(batch.people.count)
                ])
                return JSONHTTPResponse.make(ClassificationResponseDTO(batch: batch))
            } catch PersonInferenceCoordinator.ServiceError.invalidImage {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "invalid_image"), statusCode: .badRequest
                )
            } catch PersonInferenceCoordinator.ServiceError.modelUnavailable {
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
}
