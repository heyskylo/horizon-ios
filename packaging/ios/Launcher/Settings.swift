import SwiftUI
import UIKit

/// One option of the game's options.txt, as `options-get` describes it: its value and what it accepts.
struct GameOption {
    var type: String
    var value: Any?
    var min: Double?
    var max: Double?
    var step: Double?
    var choices: [String]

    init?(_ json: [String: Any]) {
        guard let type = json["type"] as? String else { return nil }
        self.type = type
        value = json["value"]
        min = json["min"] as? Double
        max = json["max"] as? Double
        step = json["step"] as? Double
        choices = json["choices"] as? [String] ?? []
    }

    var number: Double { (value as? Double) ?? Double(value as? Int ?? 0) }
    var flag: Bool { value as? Bool ?? false }
    var text: String { value as? String ?? "" }
}

/// The options a phone player reaches for, read from and written to the game itself (`options-get`,
/// `options-set`): a change applies at once and is saved, as the game's own Options screen does it.
@MainActor
final class GameOptions: ObservableObject {
    typealias Request = @MainActor (String) async -> [String: Any]
    @Published private(set) var options: [String: GameOption] = [:]
    @Published private(set) var loading = false
    @Published private(set) var issue: String?
    @Published private(set) var unavailable: [String] = []
    private let request: Request

    init(request: @escaping Request = { await Bridge.send($0) }) { self.request = request }

    func load() async {
        loading = true
        issue = nil
        defer { loading = false }
        let rows = Self.groups.flatMap(\.rows)
        let keys = rows.map(\.key)
        let answer = await request("options-get " + keys.joined(separator: " "))
        var described = answer["options"] as? [String: [String: Any]]
        // One unsupported key must not hide every supported control on a different engine revision.
        if let error = answer.error, error.hasPrefix("unknown option") {
            var supported: [String: [String: Any]] = [:]
            for key in keys {
                let one = await request("options-get " + key)
                if let value = (one["options"] as? [String: [String: Any]])?[key] { supported[key] = value }
            }
            described = supported
        } else if let error = answer.error {
            issue = error
            return
        }
        let parsed = (described ?? [:]).compactMapValues(GameOption.init)
            .filter { keys.contains($0.key) }
        guard !parsed.isEmpty else {
            issue = "The launcher did not receive any settings. Try loading them again."
            return
        }
        options = parsed
        unavailable = rows.filter { parsed[$0.key] == nil }.map(\.title)
    }

    func set(_ key: String, _ value: String) async -> String? {
        let answer = await request("options-set \(key) \(value)")
        if let error = answer.error { return error }
        guard let now = (answer["option"] as? [String: Any]).flatMap(GameOption.init) else {
            return "The launcher could not confirm that the setting was saved. Try again."
        }
        options[key] = now
        return nil
    }

    // MARK: What is shown

    struct Row {
        let key: String
        let title: String
        /// How a value reads, for sliders and numbered choices.
        var format: (Double) -> String = { String(Int($0)) }
        /// Names for an option whose values are numbers (`particles`), or for an enum's values.
        var names: [String: String] = [:]
        /// A slider's ceiling when the option's own is more than a phone should be offered.
        var cap: Double?
    }

    struct Category {
        let title: String
        let icon: String
        let rows: [Row]
    }

    static let groups: [Category] = [
        Category(title: "Video", icon: "display", rows: [
            Row(key: "graphicsPreset", title: "Graphics",
                names: ["fast": "Fast", "fancy": "Fancy", "fabulous": "Fabulous!", "custom": "Custom"]),
            Row(key: "renderDistance", title: "Render Distance", format: { "\(Int($0)) chunks" }, cap: 32),
            Row(key: "simulationDistance", title: "Simulation Distance", format: { "\(Int($0)) chunks" }),
            Row(key: "maxFps", title: "Max Framerate", format: { $0 >= 260 ? "Unlimited" : "\(Int($0)) fps" }),
            Row(key: "fov", title: "FOV", format: { $0 == 70 ? "Normal" : $0 == 110 ? "Quake Pro" : "\(Int($0))" }),
            Row(key: "gamma", title: "Brightness",
                format: { $0 <= 0 ? "Moody" : $0 >= 1 ? "Bright" : "+\(Int($0 * 100))%" }),
            Row(key: "guiScale", title: "GUI Scale", format: { $0 == 0 ? "Auto" : "\(Int($0))" }, cap: 6),
            Row(key: "particles", title: "Particles", names: ["0": "All", "1": "Decreased", "2": "Minimal"]),
            Row(key: "renderClouds", title: "Clouds", names: ["false": "Off", "fast": "Fast", "true": "Fancy"]),
            Row(key: "entityShadows", title: "Entity Shadows"),
            Row(key: "bobView", title: "View Bobbing"),
        ]),
        Category(title: "Sound", icon: "speaker.wave.2.fill", rows: [
            Row(key: "soundCategory_master", title: "Master Volume", format: GameOptions.percent),
            Row(key: "soundCategory_music", title: "Music", format: GameOptions.percent),
            Row(key: "soundCategory_ambient", title: "Ambient/Environment", format: GameOptions.percent),
            Row(key: "showSubtitles", title: "Show Subtitles"),
        ]),
        Category(title: "Controls", icon: "hand.draw.fill", rows: [
            Row(key: "mouseSensitivity", title: "Look Sensitivity", format: { "\(Int($0 * 200))%" }),
            Row(key: "autoJump", title: "Auto-Jump"),
            Row(key: "invertYMouse", title: "Invert Look"),
        ]),
    ]

    private static func percent(_ value: Double) -> String {
        value <= 0 ? "Off" : "\(Int((value * 100).rounded()))%"
    }
}

struct SettingsPage: View {
    @ObservedObject var link: GameLink
    @ObservedObject var download: GameFilesDownload
    @State private var showingNews = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            SectionHeader(title: "Game files")
            GameFilesCard(download: download, missing: !GameFolder.hasGameFiles)

            SectionHeader(title: "About")
            SecondaryButton(title: "What's New", icon: "sparkles") { showingNews = true }
            About(version: link.version)
        }
        .sheet(isPresented: $showingNews) {
            ScrollView {
                NewsPage().pageColumn()
            }
            .background(Theme.background.ignoresSafeArea())
            .presentationDetents([.large])

        }
    }
}

private struct About: View {
    let version: String

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            line("Horizon for iPhone and iPad", Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "")
            line("Build", Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "")
            line("Minecraft: Java Edition", version)
            line("Game files", GameFolder.hasGameFiles ? "Installed" : "Not installed")
            Link(destination: URL(string: "https://github.com/heyskylo/horizon-ios")!) {
                Label("Source code on GitHub", systemImage: "chevron.left.forwardslash.chevron.right")
                    .font(Theme.sans(13, .semiBold)).foregroundStyle(Theme.accentText)
            }
            Text("Your worlds, screenshots and options.txt are in the Files app, under On My iPhone › Horizon.")
                .font(Theme.sans(11)).foregroundStyle(Theme.textMuted)
            Text("Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.")
                .font(Theme.sans(11)).foregroundStyle(Theme.textFaint)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .card()
    }

    private func line(_ title: String, _ value: String) -> some View {
        HStack {
            Text(title).font(Theme.sans(13)).foregroundStyle(Theme.textMuted)
            Spacer()
            Text(value).font(Theme.mono(11)).foregroundStyle(Theme.text)
        }
    }
}
