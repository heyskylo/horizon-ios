import SwiftUI
import UIKit

/// Only diagnostic text is exported; never account stores, options or game data.
enum Diagnostics {
    static let limit = 1024 * 1024
    private static let lock = NSLock()
    static var directory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("HorizonDiagnostics", isDirectory: true)
    }
    private static let patterns: [NSRegularExpression] = [#"https?://[^\s]+"#, #"\beyJ[A-Za-z0-9_.-]+"#, #"[A-Za-z0-9_./+=-]{129,}"#, #"[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}"#, #"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?\b"#, #"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b"#, #"/(?:Users|var/mobile)/[^\s]+"#].compactMap {try? NSRegularExpression(pattern:$0,options:.caseInsensitive)}
    static func redact(_ text: String) -> String {
        let sensitive = ["accesstoken", "refreshtoken", "idtoken", "token =", "token :", "user code", "access_token", "refresh_token", "id_token", "token:", "token=", "token\"", "authorization", "bearer", "password", "secret", "cookie", "xbl3.0", "device_code", "user_code", "sessionid", "session_id", "api_key", "accesskey"]
        if sensitive.contains(where: {text.lowercased().contains($0)}) {return "[redacted sensitive log message]"}
        return text.components(separatedBy: .newlines).map { line in
            if sensitive.contains(where: { line.lowercased().contains($0) }) { return "[redacted sensitive log line]" }
            var safe = line
            for regex in patterns {
                safe = regex.stringByReplacingMatches(in:safe,range:NSRange(safe.startIndex...,in:safe),withTemplate:"[redacted]")
            }
            return safe
        }.joined(separator: "\n")
    }
    static func bounded(_ text: String, maximum: Int = limit) -> Data {
        var bytes = Data(text.utf8)
        if bytes.count > maximum {
            bytes = bytes.prefix(maximum - 32)
            // Decode/re-encode safely when the byte boundary splits Unicode.
            bytes = Data((String(decoding: bytes, as: UTF8.self) + "\n[truncated]\n").utf8)
        }
        return bytes
    }
    static func record(_ message: String, folder: URL = directory) {
        lock.lock(); defer { lock.unlock() }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let current = folder.appendingPathComponent("launcher.log")
            let previous = folder.appendingPathComponent("launcher-previous.log")
            let data = bounded("\(ISO8601DateFormatter().string(from: Date())) \(redact(message))\n", maximum: 32 * 1024)
            if !FileManager.default.fileExists(atPath: current.path) { try Data().write(to: current) }
            let size = (try current.resourceValues(forKeys: [.fileSizeKey])).fileSize ?? 0
            if size + data.count > limit {
                let tail = try read(current)
                try bounded(redact(tail)).write(to: previous, options: .atomic)
                try Data().write(to: current)
            }
            let file = try FileHandle(forWritingTo: current);defer {try? file.close()}
            try file.seekToEnd();try file.write(contentsOf: data)
        } catch { /* Logging must never interrupt gameplay or recurse into logging. */ }
    }
    static func read(_ url: URL) throws -> String {
        let file = try FileHandle(forReadingFrom: url); defer {try? file.close()}
        let length = try file.seekToEnd()
        try file.seek(toOffset: length > UInt64(limit) ? length - UInt64(limit) : 0)
        let data = try file.read(upToCount: limit) ?? Data()
        let text = String(decoding: data, as: UTF8.self)
        return length > UInt64(limit) ? String(text.drop(while: {$0 != "\n"}).dropFirst()) : text
    }
    static func redactExport(_ text:String) -> String {
        var hidingContinuation = false
        return text.components(separatedBy:.newlines).map { line in
            // Rust entries start with [time]; launcher entries start with an ISO date.
            if line.hasPrefix("[") || line.range(of:#"^\d{4}-\d{2}-\d{2}T"#,options:.regularExpression) != nil {hidingContinuation = false}
            let safe = redact(line)
            if safe == "[redacted sensitive log message]" {hidingContinuation = true;return safe}
            return hidingContinuation ? "[redacted continuation]" : safe
        }.joined(separator:"\n")
    }
    static func export(folder: URL = directory, gameFolder: URL? = nil) throws -> URL {
        // Export reads bounded snapshots without holding the launcher writer lock.
        let fm = FileManager.default
        let exportDir = folder.appendingPathComponent("Export", isDirectory: true)
        try fm.createDirectory(at: exportDir, withIntermediateDirectories: true)
        let destination = exportDir.appendingPathComponent("Horizon-diagnostics.txt")
        var output = "Horizon mobile diagnostics\nVersion: \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") ?? "unknown") Build: \(Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") ?? "unknown")\niOS: \(ProcessInfo.processInfo.operatingSystemVersionString)\nSensitive lines and common identifiers are redacted. Review before sharing.\n"
        let gameHome = gameFolder ?? ProcessInfo.processInfo.environment["HORIZON_HOME"].map {URL(fileURLWithPath:$0)} ?? fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("horizon")
        for (label,url) in [("Launcher previous",folder.appendingPathComponent("launcher-previous.log")),("Launcher current",folder.appendingPathComponent("launcher.log")),("Game previous",gameHome.appendingPathComponent("logs/previous.log")),("Game current",gameHome.appendingPathComponent("logs/latest.log"))] {
            output += "\n--- \(label) ---\n"
            if fm.fileExists(atPath:url.path) {output += String(decoding: bounded(redactExport(try read(url))), as:UTF8.self)}
            else {output += "No log recorded yet.\n"}
        }
        try bounded(output, maximum: 4 * limit).write(to:destination,options:.atomic)
        return destination
    }
}

struct LogsSettings: View {
    @State private var exportURL: URL?
    @State private var exporting = false
    @State private var error: String?
    var body: some View {
        VStack(alignment:.leading,spacing:12) {
            SectionHeader(title:"Logs")
            Button {
                exporting = true;error = nil
                Task {
                    do {exportURL = try await Task.detached(priority:.utility) {try Diagnostics.export()}.value}
                    catch {self.error = "Could not export logs. Please try again."}
                    exporting = false
                }
            } label: {Label(exporting ? "Preparing logs…" : "Export log file",systemImage:"square.and.arrow.up")}
            .disabled(exporting).accessibilityIdentifier("export-diagnostics")
            if let error {Text(error).foregroundStyle(.red)}
        }.padding(16).card()
        .sheet(isPresented: Binding(get:{exportURL != nil},set:{if !$0 {exportURL = nil}})) {
            if let exportURL {DiagnosticFileExport(url:exportURL) {self.exportURL = nil}}
        }
    }
}
private struct DiagnosticFileExport: UIViewControllerRepresentable {
    let url: URL
    let finish: () -> Void
    func makeCoordinator() -> Coordinator {Coordinator(finish:finish)}
    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forExporting:[url],asCopy:true)
        picker.delegate = context.coordinator
        picker.view.accessibilityIdentifier = "diagnostic-file-export"
        return picker
    }
    func updateUIViewController(_ controller:UIDocumentPickerViewController,context:Context) {}
    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let finish: () -> Void
        init(finish:@escaping () -> Void) {self.finish = finish}
        func documentPickerWasCancelled(_ controller:UIDocumentPickerViewController) {finish()}
        func documentPicker(_ controller:UIDocumentPickerViewController,didPickDocumentsAt urls:[URL]) {finish()}
    }
}
