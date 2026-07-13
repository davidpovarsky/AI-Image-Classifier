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
                LabeledContent("Model", value: server.modelName)
            }

            Section("Authentication") {
                Text(server.token)
                    .font(.system(.footnote, design: .monospaced))
                    .textSelection(.enabled)

                Button("Copy URL") {
                    UIPasteboard.general.url = server.localURL
                }

                Button("Copy Token") {
                    UIPasteboard.general.string = server.token
                }
            }

            Section("Controls") {
                Button(server.isRunning || server.starting ? "Stop" : "Start") {
                    if server.isRunning || server.starting {
                        server.stop()
                    } else {
                        server.start()
                    }
                }
            }

            if let lastError = server.lastError {
                Section("Last Error") {
                    Text(lastError)
                        .foregroundStyle(.red)
                }
            }
        }
        .navigationTitle("Local Server")
        .onAppear {
            server.start()
        }
        .onChange(of: scenePhase) { _, newPhase in
            if newPhase == .active, !server.isRunning, !server.starting {
                server.start()
            }
        }
    }

    private var statusText: String {
        switch server.state {
        case .stopped: "Stopped"
        case .starting: "Starting"
        case .running: "Running"
        case .failed: "Failed"
        }
    }
}
