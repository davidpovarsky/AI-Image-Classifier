import Foundation

nonisolated struct ClassificationResponseDTO: Codable, Equatable, Sendable {
    let success: Bool
    let model: String
    let serverVersion: Int
    let durationMs: Int
    let imageWidth: Int
    let imageHeight: Int
    let peopleCount: Int
    let people: [PersonClassification]

    init(batch: PersonClassificationBatch) {
        success = true
        model = LocalServerConfiguration.modelName
        serverVersion = LocalServerConfiguration.serverVersion
        durationMs = batch.inferenceDurationMs
        imageWidth = batch.imageWidth
        imageHeight = batch.imageHeight
        peopleCount = batch.people.count
        people = batch.people
    }
}

nonisolated struct HealthResponseDTO: Codable, Equatable, Sendable {
    let status: String
    let serverVersion: Int
    let model: String
    let modelLoaded: Bool
    let imageEncoderLoaded: Bool
    let textEncoderBundled: Bool
    let promptEmbeddingsReady: Bool
    let selectedComputeUnits: String?
    let modelPrecision: String
    let humanDetector: String
    let nudeNet: NudeNetHealthDTO
    let imageSafetyPipeline: ImageSafetyAvailabilityDTO
    let error: String?
}

nonisolated struct NudeNetHealthDTO: Codable, Equatable, Sendable {
    let bundled: Bool
    let loaded: Bool
    let modelName: String
    let computeUnits: String
    let labelsCount: Int
}

nonisolated struct ImageSafetyAvailabilityDTO: Codable, Equatable, Sendable {
    let available: Bool
    let pipelineVersion: Int
    let endpoint: String
}

nonisolated struct DiagnosticsStatusDTO: Codable, Equatable, Sendable {
    let sessionId: String
    let state: String
    let stage: String
    let model: String
    let precision: String
    let deploymentTarget: String
    let selectedComputeUnits: String?
    let attempts: [ModelLoadAttempt]
    let logFilesAvailable: Bool
    let imageSafetyPipeline: ImageSafetyDiagnosticsStatusDTO
}

nonisolated struct ImageSafetyDiagnosticsStatusDTO: Codable, Equatable, Sendable {
    let available: Bool
    let pipelineVersion: Int
    let lastRequestId: String?
    let lastStatus: PipelineStatus?
    let lastTotalDurationMs: Int?
    let modules: [String: ImageSafetyModuleStatusDTO]
}

nonisolated struct ImageSafetyModuleStatusDTO: Codable, Equatable, Sendable {
    let status: String
    let lastDurationMs: Int?
}

nonisolated struct ErrorResponseDTO: Codable, Equatable, Sendable {
    let success: Bool
    let error: String
}

nonisolated enum LocalAPIContract {
    static func isAuthorized(header: String?, token: String) -> Bool {
        header == "Bearer \(token)"
    }

    static func health(
        from snapshot: MobileCLIPServiceMetrics,
        nudeNet: NudeNetServiceMetrics = NudeNetServiceMetrics()
    ) -> HealthResponseDTO {
        let ready = snapshot.state == .ready
            && snapshot.imageEncoderLoaded
            && snapshot.promptEmbeddingsReady
            && snapshot.smokeTestPassed
        let error: String?
        if case .failed(let message) = snapshot.state { error = message } else { error = nil }
        return HealthResponseDTO(
            status: ready ? "ok" : "error",
            serverVersion: LocalServerConfiguration.serverVersion,
            model: LocalServerConfiguration.modelName,
            modelLoaded: ready,
            imageEncoderLoaded: snapshot.imageEncoderLoaded,
            textEncoderBundled: false,
            promptEmbeddingsReady: snapshot.promptEmbeddingsReady,
            selectedComputeUnits: snapshot.selectedComputeUnits,
            modelPrecision: "float16",
            humanDetector: "VNDetectHumanRectanglesRequest",
            nudeNet: NudeNetHealthDTO(
                bundled: true,
                loaded: nudeNet.state == .ready,
                modelName: "NudeNet320n",
                computeUnits: "all",
                labelsCount: NudeNetLabels.all.count
            ),
            imageSafetyPipeline: ImageSafetyAvailabilityDTO(
                available: true,
                pipelineVersion: ImageSafetyPipelineService.pipelineVersion,
                endpoint: "/v1/image-safety-classify"
            ),
            error: error
        )
    }
}
