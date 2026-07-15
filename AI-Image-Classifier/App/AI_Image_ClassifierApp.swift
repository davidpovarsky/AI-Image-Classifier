//
//  AI_Image_ClassifierApp.swift
//  AI-Image-Classifier
//
//  Created by Goutam Roy on 14/04/26.
//

import SwiftUI
import UIKit

@main
struct AI_Image_ClassifierApp: App {
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .task { try? await DiagnosticLogService.shared.startSession() }
                .onReceive(NotificationCenter.default.publisher(for: UIApplication.didReceiveMemoryWarningNotification)) { _ in
                    Task { try? await DiagnosticLogService.shared.log(level: "warning", category: "system", event: "memoryWarningReceived") }
                }
                .onReceive(NotificationCenter.default.publisher(for: ProcessInfo.thermalStateDidChangeNotification)) { _ in
                    Task { try? await DiagnosticLogService.shared.log(
                        level: "info", category: "system", event: "thermalStateChanged",
                        details: ["state": String(ProcessInfo.processInfo.thermalState.rawValue)]
                    ) }
                }
                .onChange(of: scenePhase) { _, phase in
                    Task { try? await DiagnosticLogService.shared.log(level: "info", category: "lifecycle", event: "scenePhaseChanged", details: ["phase": String(describing: phase)]) }
                }
        }
    }
}
