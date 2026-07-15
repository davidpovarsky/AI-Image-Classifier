import SwiftUI
import UIKit

struct LocalServerView: View {
    @State private var server = LocalInferenceServer()
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Form {
            Section("Server") {
                LabeledContent("Status", value: statusText)
                LabeledContent("Address", value: LocalServerConfiguration.host)
                LabeledContent("Port", value: String(server.configuration.port))
                LabeledContent("Model", value: LocalServerConfiguration.modelName)
            }
            Section("Model readiness") {
                LabeledContent("Image encoder", value: readiness(server.metricsSnapshot.model.imageEncoderLoaded))
                LabeledContent("Prompt embeddings", value: readiness(server.metricsSnapshot.model.promptEmbeddingsReady))
                LabeledContent("Runtime configuration", value: server.metricsSnapshot.model.selectedComputeUnits ?? "Not selected")
                LabeledContent("Stage", value: server.metricsSnapshot.model.stage)
                LabeledContent("Smoke test", value: readiness(server.metricsSnapshot.model.smokeTestPassed))
                LabeledContent("Model load", value: duration(server.metricsSnapshot.model.loadDurationMs))
            }
            if !server.metricsSnapshot.model.loadAttempts.isEmpty {
                Section("Load attempts") {
                    ForEach(server.metricsSnapshot.model.loadAttempts) { attempt in
                        LabeledContent(attempt.computeUnits, value: attempt.succeeded ? "Success — \(attempt.durationMs) ms" : "Failed — \(attempt.errorDomain ?? "unknown") \(attempt.errorCode.map(String.init) ?? "")")
                    }
                }
            }
            Section("Requests") {
                LabeledContent("Processed images", value: String(server.metricsSnapshot.totalProcessed))
                LabeledContent("Detected people", value: String(server.metricsSnapshot.totalDetectedPeople))
                LabeledContent("Average latency", value: duration(server.metricsSnapshot.averageLatencyMs))
                LabeledContent("Last latency", value: duration(server.metricsSnapshot.lastRequestLatencyMs))
                LabeledContent("Last result", value: server.metricsSnapshot.lastResultSummary ?? "None")
            }
            Section("Authentication") {
                Text(server.token).font(.system(.footnote, design: .monospaced)).textSelection(.enabled)
                Button("Copy URL") { UIPasteboard.general.url = server.localURL }
                Button("Copy Token") { UIPasteboard.general.string = server.token }
            }
            Section("Controls") {
                Button(server.isRunning || server.starting ? "Stop" : "Start") {
                    server.isRunning || server.starting ? server.stop() : server.start()
                }
                Button("Retry Model Load") { server.retryModelLoad() }
                NavigationLink("View Diagnostics") { DiagnosticsView() }
            }
            if let lastError = server.lastError {
                Section("Last Error") {
                    Text(lastError).foregroundStyle(.red)
                    Button("Copy Error") { UIPasteboard.general.string = lastError }
                }
            }
        }
        .navigationTitle("Local Server")
        .onAppear { server.start() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active, !server.isRunning, !server.starting { server.start() }
        }
        .task {
            while !Task.isCancelled {
                await server.refreshMetrics()
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    private var statusText: String {
        switch server.metricsSnapshot.model.state {
        case .notLoaded: server.state == .starting ? "Starting server…" : "Loading model…"
        case .loading: "Loading model…"
        case .ready: "Ready"
        case .failed(let message): "Failed: \(message)"
        }
    }

    private func readiness(_ ready: Bool) -> String { ready ? "Ready" : "Not ready" }
    private func duration(_ milliseconds: Int?) -> String { milliseconds.map { "\($0) ms" } ?? "—" }
}
