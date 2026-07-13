import FlyingFox
import FlyingSocks
import Foundation
import Observation

@MainActor
@Observable
final class LocalInferenceServer {
    enum State: Equatable {
        case stopped
        case starting
        case running
        case failed
    }

    private(set) var state: State = .stopped
    private(set) var lastError: String?

    let configuration: LocalServerConfiguration
    let modelName = LocalServerConfiguration.modelName

    private let server: HTTPServer
    private let classifier = CoreMLImageClassifier.shared
    private var lifecycleTask: Task<Void, Never>?
    private var lifecycleID: UUID?
    private var routesConfigured = false

    init(configuration: LocalServerConfiguration = .makeDefault()) {
        self.configuration = configuration
        let address: sockaddr_in
        do {
            address = try sockaddr_in.inet(
                ip4: LocalServerConfiguration.host,
                port: configuration.port
            )
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
        guard let url = components.url else {
            preconditionFailure("The built-in local server URL is invalid.")
        }
        return url
    }

    func start() {
        guard lifecycleTask == nil, !running, !starting else { return }

        state = .starting
        lastError = nil

        let lifecycleID = UUID()
        self.lifecycleID = lifecycleID
        lifecycleTask = Task { [weak self] in
            guard let self else { return }
            await self.runServer(lifecycleID: lifecycleID)
        }
    }

    func stop() {
        guard lifecycleTask != nil || running || starting else {
            state = .stopped
            return
        }

        let previousTask = lifecycleTask
        previousTask?.cancel()
        let lifecycleID = UUID()
        self.lifecycleID = lifecycleID
        state = .stopped
        lifecycleTask = Task { [weak self] in
            guard let self else { return }
            await server.stop()
            if self.lifecycleID == lifecycleID {
                self.lifecycleID = nil
                self.lifecycleTask = nil
            }
        }
    }

    func restart() {
        let previousTask = lifecycleTask
        previousTask?.cancel()
        let lifecycleID = UUID()
        self.lifecycleID = lifecycleID
        state = .starting
        lastError = nil
        lifecycleTask = Task { [weak self] in
            guard let self else { return }
            await server.stop()
            if self.lifecycleID == lifecycleID {
                self.lifecycleID = nil
                self.lifecycleTask = nil
                self.start()
            }
        }
    }

    private func runServer(lifecycleID: UUID) async {
        await configureRoutesIfNeeded()

        let runTask = Task {
            try await server.run()
        }

        do {
            try await server.waitUntilListening()
            try Task.checkCancellation()
            guard self.lifecycleID == lifecycleID else {
                runTask.cancel()
                return
            }
            self.state = .running
            try await runTask.value
            if !Task.isCancelled, self.lifecycleID == lifecycleID {
                self.state = .stopped
            }
        } catch is CancellationError {
            runTask.cancel()
            await server.stop()
            if self.lifecycleID == lifecycleID {
                self.state = .stopped
            }
        } catch {
            runTask.cancel()
            await server.stop()
            if self.lifecycleID == lifecycleID {
                if Task.isCancelled {
                    self.state = .stopped
                } else {
                    self.lastError = "The local server could not start or remain active."
                    self.state = .failed
                }
            }
        }

        if self.lifecycleID == lifecycleID {
            self.lifecycleID = nil
            self.lifecycleTask = nil
        }
    }

    private func configureRoutesIfNeeded() async {
        guard !routesConfigured else { return }
        routesConfigured = true

        await server.appendRoute("GET /health") { _ in
            JSONHTTPResponse.make(
                HealthResponseDTO(
                    status: "ok",
                    model: LocalServerConfiguration.modelName,
                    serverVersion: LocalServerConfiguration.serverVersion
                )
            )
        }

        let configuration = configuration
        let classifier = classifier
        await server.appendRoute("POST /v1/classify") { request in
            guard request.headers[.authorization] == "Bearer \(configuration.bearerToken)" else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unauthorized"),
                    statusCode: .unauthorized
                )
            }

            let contentType = request.headers[.contentType]?
                .split(separator: ";", maxSplits: 1)
                .first?
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .lowercased()

            let supportedMediaTypes: Set<String> = [
                "image/jpeg",
                "image/png",
                "image/webp",
                "image/heic",
                "image/heif"
            ]

            guard let contentType, supportedMediaTypes.contains(contentType) else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "unsupported_media_type"),
                    statusCode: .unsupportedMediaType
                )
            }

            if let contentLength = request.headers[.contentLength],
               let byteCount = Int(contentLength),
               byteCount > configuration.maximumImageBytes {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "payload_too_large"),
                    statusCode: .payloadTooLarge
                )
            }

            let imageData: Data
            do {
                imageData = try await request.bodyData
            } catch {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "classification_failed"),
                    statusCode: .internalServerError
                )
            }
            guard imageData.count <= configuration.maximumImageBytes else {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "payload_too_large"),
                    statusCode: .payloadTooLarge
                )
            }

            let started = ContinuousClock.now
            do {
                let predictions = try await classifier.classify(imageData: imageData)
                let duration = started.duration(to: .now)
                let durationMs = Int(duration.components.seconds * 1_000)
                    + Int(duration.components.attoseconds / 1_000_000_000_000_000)

                return JSONHTTPResponse.make(
                    ClassificationResponseDTO(
                        success: true,
                        predictions: predictions,
                        durationMs: durationMs
                    )
                )
            } catch CoreMLService.ServiceError.invalidImage {
                return JSONHTTPResponse.make(
                    ErrorResponseDTO(success: false, error: "invalid_image"),
                    statusCode: .unprocessableContent
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
