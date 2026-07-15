import SwiftUI
import UIKit

struct DiagnosticsView: View {
    @State private var session: DiagnosticSessionSnapshot?
    @State private var model = MobileCLIPServiceMetrics()
    @State private var exportURL: URL?
    @State private var showFolderHelp = false

    var body: some View {
        Form {
            Section("Session") {
                LabeledContent("Session ID", value: session?.sessionID ?? "Not started")
                LabeledContent("Start time", value: session?.startedAt ?? "—")
                LabeledContent("Runtime log", value: ByteCountFormatter.string(fromByteCount: Int64(session?.runtimeLogBytes ?? 0), countStyle: .file))
                LabeledContent("Events", value: String(session?.eventCount ?? 0))
                LabeledContent("Last event", value: session?.lastEvent ?? "—")
                LabeledContent("Last error", value: session?.lastError ?? "—")
            }
            Section("Device") {
                LabeledContent("System", value: "\(UIDevice.current.systemName) \(UIDevice.current.systemVersion)")
                LabeledContent("Physical memory", value: ByteCountFormatter.string(fromByteCount: Int64(ProcessInfo.processInfo.physicalMemory), countStyle: .memory))
                LabeledContent("Thermal state", value: String(ProcessInfo.processInfo.thermalState.rawValue))
                LabeledContent("Low Power Mode", value: ProcessInfo.processInfo.isLowPowerModeEnabled ? "On" : "Off")
            }
            Section("Model") {
                LabeledContent("Identifier", value: LocalServerConfiguration.modelName)
                LabeledContent("Precision", value: "Float16")
                LabeledContent("Deployment target", value: "iOS 17")
                LabeledContent("Compute units", value: model.selectedComputeUnits ?? "None")
                LabeledContent("Load stage", value: model.stage)
                LabeledContent("Smoke test", value: model.smokeTestPassed ? "Passed" : "Not passed")
            }
            Section("Load attempts") {
                if model.loadAttempts.isEmpty { Text("No attempts") }
                ForEach(model.loadAttempts) { attempt in
                    VStack(alignment: .leading) {
                        Text(attempt.computeUnits).font(.headline)
                        Text(attempt.succeeded ? "Succeeded — \(attempt.durationMs) ms" : "Failed — \(attempt.errorDomain ?? "unknown") \(attempt.errorCode.map(String.init) ?? "")")
                            .foregroundStyle(attempt.succeeded ? .green : .red)
                    }
                }
            }
            Section("Actions") {
                Button("Open Diagnostics Folder") { showFolderHelp = true }
                Button("Export Diagnostic Bundle") {
                    Task { exportURL = try? await DiagnosticLogService.shared.exportLatestSession() }
                }
                if let exportURL { ShareLink("Share Latest Log", item: exportURL) }
                Button("Delete Old Logs") { Task { try? await DiagnosticLogService.shared.deleteOldLogs(); await refresh() } }
                Button("Run Model Load Test Again") { Task { try? await PersonInferenceCoordinator.shared.reloadModel(); await refresh() } }
                Button("Run Smoke Test Again") { Task { try? await PersonInferenceCoordinator.shared.reloadModel(); await refresh() } }
                Button("Copy Last Error") { UIPasteboard.general.string = session?.lastError }
            }
        }
        .navigationTitle("Diagnostics")
        .alert("Diagnostics Folder", isPresented: $showFolderHelp) {
            Button("OK", role: .cancel) { }
        } message: {
            Text("Files → On My iPad → AI Image Classifier → AI-Image-Classifier Diagnostics")
        }
        .task { await refresh() }
    }

    private func refresh() async {
        session = await DiagnosticLogService.shared.snapshot()
        model = await PersonInferenceCoordinator.shared.modelSnapshot()
    }
}
