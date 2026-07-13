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
                LabeledContent("Model", value: "NudeNet 320n")
                LabeledContent("Threshold profile", value: server.thresholdProfile.capitalized)
            }

            Section("Model performance") {
                LabeledContent("Model load", value: duration(server.metricsSnapshot.model.loadDurationMs))
                LabeledContent("Warm-up", value: duration(server.metricsSnapshot.model.warmUpDurationMs))
                LabeledContent("Last inference", value: duration(server.metricsSnapshot.model.lastInferenceDurationMs))
            }

            Section("Requests") {
                LabeledContent("Total processed", value: String(server.metricsSnapshot.totalProcessed))
                LabeledContent("Allowed", value: String(server.metricsSnapshot.totalAllowed))
                LabeledContent("Blocked", value: String(server.metricsSnapshot.totalBlocked))
                LabeledContent("Last triggered class", value: server.metricsSnapshot.lastTriggeredClass ?? "None")
            }

            Section("Authentication") {
                Text(server.token)
                    .font(.system(.footnote, design: .monospaced))
                    .textSelection(.enabled)
                Button("Copy URL") { UIPasteboard.general.url = server.localURL }
                Button("Copy Token") { UIPasteboard.general.string = server.token }
            }

            Section("Controls") {
                Button(server.isRunning || server.starting ? "Stop" : "Start") {
                    server.isRunning || server.starting ? server.stop() : server.start()
                }
            }

            if let lastError = server.lastError {
                Section("Last Error") { Text(lastError).foregroundStyle(.red) }
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
        case .notLoaded: server.state == .starting ? "Starting server..." : "Loading model..."
        case .loading: "Loading model..."
        case .warming: "Warming model..."
        case .ready: "Ready"
        case .failed(let message): "Failed: \(message)"
        }
    }

    private func duration(_ milliseconds: Int?) -> String {
        milliseconds.map { "\($0) ms" } ?? "—"
    }
}
