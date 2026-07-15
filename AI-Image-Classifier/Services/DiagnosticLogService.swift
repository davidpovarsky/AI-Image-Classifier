import Foundation
import CryptoKit
import Darwin
import UIKit

nonisolated struct DiagnosticSessionSnapshot: Sendable {
    let sessionID: String
    let startedAt: String
    let folderURL: URL
    let runtimeLogBytes: Int
    let eventCount: Int
    let lastEvent: String?
    let lastError: String?
}

actor DiagnosticLogService {
    static let shared = DiagnosticLogService()

    private let fileManager: FileManager
    private let documentsURL: URL
    private var sessionURL: URL?
    private var sessionID = "not-started"
    private var startedAt = ""
    private var eventCount = 0
    private var lastEvent: String?
    private var lastError: String?

    init(fileManager: FileManager = .default, documentsURL: URL? = nil) {
        self.fileManager = fileManager
        self.documentsURL = documentsURL ?? fileManager.urls(for: .documentDirectory, in: .userDomainMask).first!
    }

    func startSession() async throws {
        guard sessionURL == nil else { return }
        let now = Date()
        startedAt = Self.timestamp(now)
        sessionID = UUID().uuidString
        let safeDate = startedAt.replacingOccurrences(of: ":", with: "-")
        let root = diagnosticsRoot
        try fileManager.createDirectory(at: root.appendingPathComponent("Current", isDirectory: true), withIntermediateDirectories: true)
        try fileManager.createDirectory(at: root.appendingPathComponent("Exports", isDirectory: true), withIntermediateDirectories: true)
        let folder = root
            .appendingPathComponent("Sessions", isDirectory: true)
            .appendingPathComponent("\(safeDate)_\(sessionID)", isDirectory: true)
        try fileManager.createDirectory(at: folder, withIntermediateDirectories: true)
        sessionURL = folder
        for name in ["runtime.log", "events.jsonl", "health-snapshots.jsonl", "inference-events.jsonl"] {
            try Data().write(to: folder.appendingPathComponent(name), options: .atomic)
        }
        try Data("[]\n".utf8).write(to: folder.appendingPathComponent("model-load-attempts.json"), options: .atomic)
        try Data("{}\n".utf8).write(to: folder.appendingPathComponent("summary.json"), options: .atomic)
        try writeJSON(await Self.deviceMetadata(), named: "device.json")
        try writeJSON(Self.appMetadata(), named: "app.json")
        if let manifest = Bundle.main.url(forResource: "MobileCLIP2S2ModelManifest", withExtension: "json") {
            try fileManager.copyItem(at: manifest, to: folder.appendingPathComponent("model-manifest.json"))
        } else {
            try Data("{}\n".utf8).write(to: folder.appendingPathComponent("model-manifest.json"), options: .atomic)
        }
        try await log(level: "info", category: "lifecycle", event: "applicationLaunched")
        try rotateIfNeeded()
    }

    func log(level: String, category: String, event: String, details: [String: String] = [:]) async throws {
        if sessionURL == nil { try await startSession() }
        guard let sessionURL else { return }
        let timestamp = Self.timestamp()
        let redacted = details.mapValues(Self.redact)
        let record: [String: Any] = [
            "timestamp": timestamp, "level": level, "category": category,
            "event": event, "details": redacted
        ]
        let json = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys]) + Data([0x0A])
        try append(json, to: sessionURL.appendingPathComponent("events.jsonl"))
        let suffix = redacted.isEmpty ? "" : " \(redacted.sorted { $0.key < $1.key })"
        let line = "[\(timestamp)] [\(level.uppercased())] \(event)\(suffix)\n"
        try append(Data(line.utf8), to: sessionURL.appendingPathComponent("runtime.log"))
        eventCount += 1
        lastEvent = event
        if level == "error" { lastError = redacted["description"] ?? event }
        try rotateRuntimeLogIfNeeded()
    }

    func recordLoadAttempts(_ attempts: [ModelLoadAttempt]) throws {
        try writeJSON(attempts, named: "model-load-attempts.json")
    }

    func appendHealth<T: Encodable>(_ value: T) throws { try appendJSONLine(value, named: "health-snapshots.jsonl") }
    func appendInference<T: Encodable>(_ value: T) throws { try appendJSONLine(value, named: "inference-events.jsonl") }

    func snapshot() -> DiagnosticSessionSnapshot? {
        guard let sessionURL else { return nil }
        let size = (try? fileManager.attributesOfItem(atPath: sessionURL.appendingPathComponent("runtime.log").path(percentEncoded: false))[.size] as? NSNumber)?.intValue ?? 0
        return DiagnosticSessionSnapshot(
            sessionID: sessionID, startedAt: startedAt, folderURL: sessionURL,
            runtimeLogBytes: size, eventCount: eventCount, lastEvent: lastEvent, lastError: lastError
        )
    }

    func exportLatestSession() throws -> URL {
        guard let sessionURL else { throw CocoaError(.fileNoSuchFile) }
        let export = diagnosticsRoot
            .appendingPathComponent("Exports", isDirectory: true)
            .appendingPathComponent("MobileCLIP2-Diagnostics-\(startedAt.replacingOccurrences(of: ":", with: "-")).diagnostics", isDirectory: true)
        try? fileManager.removeItem(at: export)
        try fileManager.copyItem(at: sessionURL, to: export)
        return export
    }

    func deleteOldLogs() throws { try rotate(keepNewest: 3, maximumSessions: 3, maximumBytes: 100 * 1_024 * 1_024) }

    private var diagnosticsRoot: URL {
        documentsURL.appendingPathComponent("AI-Image-Classifier Diagnostics", isDirectory: true)
    }

    private func writeJSON<T: Encodable>(_ value: T, named name: String) throws {
        guard let sessionURL else { return }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(value).write(to: sessionURL.appendingPathComponent(name), options: .atomic)
    }

    private func appendJSONLine<T: Encodable>(_ value: T, named name: String) throws {
        guard let sessionURL else { return }
        try append(try JSONEncoder().encode(value) + Data([0x0A]), to: sessionURL.appendingPathComponent(name))
    }

    private func append(_ data: Data, to url: URL) throws {
        let handle = try FileHandle(forWritingTo: url)
        defer { try? handle.close() }
        try handle.seekToEnd()
        try handle.write(contentsOf: data)
        try handle.synchronize()
    }

    private func rotateRuntimeLogIfNeeded() throws {
        guard let sessionURL else { return }
        let url = sessionURL.appendingPathComponent("runtime.log")
        let size = (try fileManager.attributesOfItem(atPath: url.path(percentEncoded: false))[.size] as? NSNumber)?.intValue ?? 0
        guard size > 10 * 1_024 * 1_024 else { return }
        let data = try Data(contentsOf: url)
        try data.suffix(5 * 1_024 * 1_024).write(to: url, options: .atomic)
    }

    private func rotateIfNeeded() throws { try rotate(keepNewest: 3, maximumSessions: 20, maximumBytes: 100 * 1_024 * 1_024) }

    private func rotate(keepNewest: Int, maximumSessions: Int, maximumBytes: Int) throws {
        let sessions = diagnosticsRoot.appendingPathComponent("Sessions", isDirectory: true)
        let urls = try fileManager.contentsOfDirectory(at: sessions, includingPropertiesForKeys: [.contentModificationDateKey, .totalFileAllocatedSizeKey])
            .filter { $0 != sessionURL }.sorted { $0.lastPathComponent > $1.lastPathComponent }
        var total = urls.reduce(0) { $0 + Self.directorySize($1, fileManager: fileManager) }
        for (index, url) in urls.enumerated().reversed() where index >= keepNewest && (urls.count + 1 > maximumSessions || total > maximumBytes) {
            let size = Self.directorySize(url, fileManager: fileManager)
            try fileManager.removeItem(at: url)
            total -= size
        }
    }

    nonisolated static func flattenedErrors(_ error: Error, maximumDepth: Int = 8) -> [DiagnosticError] {
        var result: [DiagnosticError] = []
        func visit(_ current: Error, depth: Int) {
            guard depth < maximumDepth else { return }
            let ns = current as NSError
            result.append(DiagnosticError(
                domain: ns.domain, code: ns.code, description: ns.localizedDescription,
                failureReason: ns.localizedFailureReason, recoverySuggestion: ns.localizedRecoverySuggestion,
                userInfo: ns.userInfo.reduce(into: [:]) { $0[String(describing: $1.key)] = redact(String(describing: $1.value)) }
            ))
            if let underlying = ns.userInfo[NSUnderlyingErrorKey] as? Error { visit(underlying, depth: depth + 1) }
            if let multiple = ns.userInfo[NSMultipleUnderlyingErrorsKey] as? [Error] {
                multiple.forEach { visit($0, depth: depth + 1) }
            }
        }
        visit(error, depth: 0)
        return result
    }

    nonisolated static func redact(_ value: String) -> String {
        var result = value.replacingOccurrences(of: #"(?i)Bearer\s+[A-Za-z0-9._~+/-]+"#, with: "Bearer <REDACTED>", options: .regularExpression)
        result = result.replacingOccurrences(of: #"/Bundle/Application/[0-9A-Fa-f-]+/"#, with: "/Bundle/Application/<APP_CONTAINER>/", options: .regularExpression)
        return result
    }

    private nonisolated static func timestamp(_ date: Date = Date()) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: date)
    }
    private nonisolated static func directorySize(_ url: URL, fileManager: FileManager) -> Int {
        let values = fileManager.enumerator(at: url, includingPropertiesForKeys: [.fileSizeKey])?.compactMap { $0 as? URL }.compactMap { try? $0.resourceValues(forKeys: [.fileSizeKey]).fileSize } ?? []
        return values.reduce(0, +)
    }

    @MainActor private static func deviceMetadata() -> [String: String] {
        let device = UIDevice.current
        let process = ProcessInfo.processInfo
        let screenScale = UIApplication.shared.connectedScenes
            .compactMap { ($0 as? UIWindowScene)?.screen.scale }
            .first
            .map { String(describing: $0) } ?? "unavailable"
        return [
            "deviceName": device.userInterfaceIdiom == .pad ? "iPad" : "iPhone",
            "systemName": device.systemName, "systemVersion": device.systemVersion,
            "modelIdentifier": modelIdentifier(),
            "processorCount": String(process.processorCount), "activeProcessorCount": String(process.activeProcessorCount),
            "physicalMemoryBytes": String(process.physicalMemory), "lowPowerModeEnabled": String(process.isLowPowerModeEnabled),
            "screenScale": screenScale, "locale": Locale.current.identifier,
            "timeZone": TimeZone.current.identifier, "thermalState": String(process.thermalState.rawValue)
        ]
    }

    private nonisolated static func modelIdentifier() -> String {
        var system = utsname()
        uname(&system)
        return withUnsafePointer(to: &system.machine) {
            $0.withMemoryRebound(to: CChar.self, capacity: 1) { String(cString: $0) }
        }
    }

    private nonisolated static func appMetadata() -> [String: String] {
        let info = Bundle.main.infoDictionary ?? [:]
        var result = [
            "bundleIdentifier": Bundle.main.bundleIdentifier ?? "unknown",
            "version": info["CFBundleShortVersionString"] as? String ?? "unknown",
            "build": info["CFBundleVersion"] as? String ?? "unknown",
            "gitCommit": info["GitCommit"] as? String ?? "unknown",
            "gitBranch": info["GitBranch"] as? String ?? "unknown",
            "buildDate": info["BuildDate"] as? String ?? "unknown",
            "xcodeVersion": info["DTXcodeBuild"] as? String ?? "unknown",
            "coremltoolsVersion": info["CoreMLToolsVersion"] as? String ?? "9.0",
            "pytorchVersion": info["PyTorchVersion"] as? String ?? "2.8.0",
            "modelCheckpointSha256": "37c2d839a856491f2fcc82c40dc28672dbd0907235b4cd4c38dfff6457f0c09f"
        ]
        if let prompts = Bundle.main.url(forResource: "MobileCLIP2S2PromptEmbeddings", withExtension: "json"),
           let data = try? Data(contentsOf: prompts) {
            result["promptEmbeddingsSha256"] = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        }
        if let manifest = Bundle.main.url(forResource: "MobileCLIP2S2ModelManifest", withExtension: "json"),
           let data = try? Data(contentsOf: manifest),
           let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            result["imageEncoderSha256"] = object["imageEncoderSHA256"] as? String ?? "unknown"
        }
        return result
    }
}
