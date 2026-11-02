import Security
import SwiftUI
import UIKit

/// Prepared game files downloaded automatically on first launch, or imported from a custom link.
/// Both routes use the game's checked importer. Raw Java assets are prepared on a computer.
@MainActor
final class GameFilesDownload: NSObject, ObservableObject {
    enum State: Equatable {
        case idle
        case downloading(received: Int64, total: Int64)
        case installing
        case done(String)
        case failed(String)
    }

    @Published private(set) var state: State = .idle
    @Published var link: String = UserDefaults.standard.string(forKey: "gameFilesLink") ?? ""
    @Published var token: String = Keychain.read() ?? ""

    typealias ImportArchive = @MainActor (String) async -> [String: Any]
    private let importArchive: ImportArchive
    private let automaticSource: @MainActor () async -> [String: Any]
    private var automaticAttempted = false
    // Same packed-ZIP limit as horizon_client::import::MAX_PACKED. Enforce
    // it during transfer, before an oversized archive fills the sandbox.
    private nonisolated static let maximumArchiveBytes: Int64 = 2 << 30
    private var resourcesOnly = false
    private var resolvingAutomatic = false
    private var attempt = UUID()
    private var session: URLSession?

    init(importArchive: @escaping ImportArchive = { await Bridge.send($0) },
         automaticSource: @escaping @MainActor () async -> [String: Any] = { await Bridge.send("game-files-source") }) {
        self.importArchive = importArchive
        self.automaticSource = automaticSource
        super.init()
    }
    private var background: UIBackgroundTaskIdentifier = .invalid

    var busy: Bool {
        if resolvingAutomatic { return true }
        switch state {
        case .downloading, .installing: return true
        default: return false
        }
    }

    /// Where a finished download waits for the importer; it consumes the file.
    private nonisolated static var destination: URL {
        GameFolder.documents.appendingPathComponent("horizon-download-\(UUID().uuidString).zip")
    }

    func start() {
        guard !busy else { return }
        attempt = UUID()
        resourcesOnly = false
        let text = link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: text), let scheme = url.scheme?.lowercased(),
              scheme == "https" || scheme == "http", url.host != nil
        else {
            state = .failed("That is not a web link. It starts with https:// (or http:// on your own network).")
            return
        }
        UserDefaults.standard.set(text, forKey: "gameFilesLink")
        let token = token.trimmingCharacters(in: .whitespacesAndNewlines)
        Keychain.write(token.isEmpty ? nil : token)
        begin(url: url, token: token.isEmpty ? nil : token)
    }

    /// First-run setup stays in the launcher; it starts no game or renderer.
    func startAutomaticallyIfNeeded() async {
        guard !GameFolder.hasGameFiles, !automaticAttempted, state == .idle, !busy else { return }
        await startAutomatic()
    }

    func startAutomatic() async {
        guard !busy else { return }
        automaticAttempted = true
        resourcesOnly = true
        resolvingAutomatic = true
        attempt = UUID()
        let generation = attempt
        state = .downloading(received: 0, total: 0)
        let answer = await automaticSource()
        guard generation == attempt else { return }
        resolvingAutomatic = false
        guard !Task.isCancelled else { state = .idle; return }
        // A cable/manual import can finish while the source request is pending.
        guard !GameFolder.hasGameFiles else { state = .done("Game files are already installed."); return }
        guard let text = answer["url"] as? String, let url = URL(string: text),
              ["https", "http"].contains(url.scheme?.lowercased() ?? ""), url.host != nil else {
            state = .failed(answer.error ?? "The automatic download is unavailable. Retry or use a download link.")
            return
        }
        // A custom link's token belongs only to that link, never the public source.
        begin(url: url, token: nil)
    }

    private func begin(url: URL, token: String?) {
        var request = URLRequest(url: url)
        if let token { request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        // A prepared tree is ~600 MB: ask for the time to finish if the player looks away.
        background = UIApplication.shared.beginBackgroundTask(withName: "Horizon game files") { [weak self] in
            let download = self
            Task { @MainActor in download?.endBackground() }
        }
        let session = URLSession(configuration: .default, delegate: self, delegateQueue: nil)
        self.session = session
        state = .downloading(received: 0, total: 0)
        session.downloadTask(with: request).resume()
    }

    func cancel() {
        attempt = UUID()
        resolvingAutomatic = false
        session?.invalidateAndCancel()
        session = nil
        state = .idle
        endBackground()
    }

    private func endBackground() {
        if background != .invalid {
            UIApplication.shared.endBackgroundTask(background)
            background = .invalid
        }
    }

    private func finished(_ outcome: Result<URL, Error>, from source: URLSession) async {
        guard session === source else {
            if case let .success(file) = outcome { try? FileManager.default.removeItem(at: file) }
            return
        }
        let generation = attempt
        session?.finishTasksAndInvalidate()
        session = nil
        switch outcome {
        case let .failure(error):
            state = .failed(error.localizedDescription)
            Haptics.failed()
        case let .success(file):
            defer { try? FileManager.default.removeItem(at: file) }
            // A manual/cable import can also finish after the HTTP transfer began.
            guard !resourcesOnly || !GameFolder.hasGameFiles else {
                state = .done("Game files are already installed.")
                break
            }
            state = .installing
            let command = resourcesOnly ? "import-game-files" : "import"
            let answer = await importArchive("\(command) \(file.path)")
            guard generation == attempt else { return }
            if let error = answer.error {
                state = .failed(error)
                Haptics.failed()
            } else {
                state = .done(Self.describe(answer))
                Haptics.done()
            }
        }
        endBackground()
    }

    private static func describe(_ answer: [String: Any]) -> String {
        let name = answer["name"] as? String ?? ""
        switch answer["kind"] as? String {
        case "resources":
            return GameFolder.hasGameFiles
                ? "Game files installed. If the game was already running, close Horizon and open it again to use them."
                : "Game files installed."
        case "world": return "World \(name) added."
        case "resource_pack": return "Resource pack \(name) added."
        default: return "Installed."
        }
    }
}

extension GameFilesDownload: URLSessionDownloadDelegate {
    nonisolated func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData _: Int64,
                                totalBytesWritten: Int64, totalBytesExpectedToWrite: Int64) {
        let oversized = totalBytesWritten > Self.maximumArchiveBytes || totalBytesExpectedToWrite > Self.maximumArchiveBytes
        if oversized { downloadTask.cancel() }
        Task { @MainActor in
            guard self.session === session else { return }
            if oversized {
                self.cancel()
                self.state = .failed("The archive exceeds the 2 GiB game-file limit.")
                return
            }
            self.state = .downloading(received: totalBytesWritten, total: max(totalBytesExpectedToWrite, 0))
        }
    }

    nonisolated func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask,
                                didFinishDownloadingTo location: URL) {
        // The file at `location` is gone once this returns: move it now, on this queue.
        let outcome: Result<URL, Error>
        let status = (downloadTask.response as? HTTPURLResponse)?.statusCode ?? 200
        if downloadTask.countOfBytesReceived > Self.maximumArchiveBytes || downloadTask.countOfBytesExpectedToReceive > Self.maximumArchiveBytes {
            outcome = .failure(DownloadError("The archive exceeds the 2 GiB game-file limit."))
        } else if !(200..<300).contains(status) {
            let reason = status == 401 || status == 403
                ? "The server refused the download (\(status)): check the access token."
                : "The server answered \(status) \(HTTPURLResponse.localizedString(forStatusCode: status))."
            outcome = .failure(DownloadError(reason))
        } else {
            do {
                let destination = Self.destination
                try? FileManager.default.removeItem(at: destination)
                try FileManager.default.moveItem(at: location, to: destination)
                outcome = .success(destination)
            } catch {
                outcome = .failure(error)
            }
        }
        Task { @MainActor in await self.finished(outcome, from: session) }
    }

    nonisolated func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let error else { return }
        let failure: Error = task.countOfBytesReceived > Self.maximumArchiveBytes || task.countOfBytesExpectedToReceive > Self.maximumArchiveBytes
            ? DownloadError("The archive exceeds the 2 GiB game-file limit.") : error
        Task { @MainActor in
            guard self.session === session else { return }
            await self.finished(.failure(failure), from: session)
        }
    }
}

private struct DownloadError: LocalizedError {
    let errorDescription: String?
    init(_ reason: String) { errorDescription = reason }
}

/// The access token, kept in the keychain rather than in preferences.
private enum Keychain {
    private static let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "dev.horizon.launcher",
        kSecAttrAccount as String: "game-files-token",
    ]

    static func read() -> String? {
        var item: CFTypeRef?
        var search = query
        search[kSecReturnData as String] = true
        guard SecItemCopyMatching(search as CFDictionary, &item) == errSecSuccess, let data = item as? Data else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    static func write(_ token: String?) {
        SecItemDelete(query as CFDictionary)
        guard let token, let data = token.data(using: .utf8) else { return }
        var item = query
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        SecItemAdd(item as CFDictionary, nil)
    }
}

// MARK: - The card

/// Get the game files: the link, the token, the progress and what happened.
struct GameFilesCard: View {
    @ObservedObject var download: GameFilesDownload
    /// The game has none yet: say so first.
    var missing: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: missing ? "exclamationmark.triangle.fill" : "arrow.down.circle.fill")
                    .foregroundStyle(missing ? Theme.warn : Theme.accentText)
                Text(missing ? "Game files needed" : "Download game files")
                    .font(Theme.pixel(18)).foregroundStyle(Theme.text)
            }
            if missing {
                Text("Horizon downloads and installs the game files automatically. Keep this screen open until setup finishes.")
                    .font(Theme.sans(12)).foregroundStyle(Theme.textMuted)
            }
            progress

            if download.busy {
                SecondaryButton(title: "Cancel", action: download.cancel)
                    .disabled(download.state == .installing)
            } else {
                if missing {
                    PrimaryButton(title: "Download game files", icon: "arrow.down") {
                        Task { await download.startAutomatic() }
                    }
                    .accessibilityIdentifier("launcher-download-game-files")
                }
            }
            DisclosureGroup(missing ? "Use a download link instead" : "Download from a link") {
                Text("""
                Horizon plays with the files your own Minecraft: Java Edition makes. Prepare them on a computer \
                (horizon-prepare, then pack them as a zip), put the zip somewhere this phone can reach, such as your \
                computer on this Wi-Fi or a server of your own, and paste its link. Worlds and resource packs \
                (.zip) work too.
                """)
                .font(Theme.sans(12)).foregroundStyle(Theme.textMuted)
                .fixedSize(horizontal: false, vertical: true)

                Field(title: "LINK", placeholder: "https://example.com/horizon-26.3.zip", text: $download.link,
                      secure: false)
                Field(title: "ACCESS TOKEN (OPTIONAL)", placeholder: "Sent as a Bearer token", text: $download.token,
                      secure: true)

                PrimaryButton(title: "Download from link", icon: "arrow.down", action: download.start)
                    .disabled(download.busy || download.link.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            Text("You can also send the zip from the game's own screen over Wi-Fi, or drop it into Horizon's folder in the Files app.")
                .font(Theme.sans(11)).foregroundStyle(Theme.textFaint)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(16)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(missing ? Theme.noticeBorder : Theme.border))
    }

    @ViewBuilder private var progress: some View {
        switch download.state {
        case .idle:
            EmptyView()
        case let .downloading(received, total):
            VStack(alignment: .leading, spacing: 6) {
                if total > 0 {
                    ProgressView(value: Double(received), total: Double(total)).tint(Theme.accent)
                } else {
                    ProgressView().tint(Theme.accent)
                }
                Text(total > 0
                    ? "\(Self.size(received)) of \(Self.size(total))"
                    : "\(Self.size(received)) downloaded")
                    .font(Theme.mono(10)).foregroundStyle(Theme.textMuted)
            }
        case .installing:
            HStack(spacing: 8) {
                ProgressView().tint(Theme.accent)
                Text("Unpacking…").font(Theme.mono(10)).foregroundStyle(Theme.textMuted)
            }
        case let .done(message):
            Label(message, systemImage: "checkmark.circle.fill")
                .font(Theme.sans(12)).foregroundStyle(Theme.ready)
        case let .failed(message):
            Label(message, systemImage: "xmark.octagon.fill")
                .font(Theme.sans(12)).foregroundStyle(Theme.danger)
        }
    }

    private static func size(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    }
}
