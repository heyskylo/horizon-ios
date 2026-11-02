import SwiftUI
import UIKit

#if ORIENTATION_FIXTURE
@main
struct OrientationFixtureEntry {
    static func main() {
        UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil,
                          NSStringFromClass(OrientationFixtureAppDelegate.self))
    }
}
#endif

private let previewSubmit: HorizonSubmit = { pointer, context, done in
    let line = pointer.map { String(cString: $0) } ?? ""
#if ORIENTATION_FIXTURE
    if ProcessInfo.processInfo.environment["PREVIEW_GAME_PRESENTATION"] == "1",
       line.hasPrefix("launcher-visible ") {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
            orientationFixtureRoot?.launcherVisible = line.hasSuffix(" true")
            orientationFixtureRoot?.setNeedsUpdateOfSupportedInterfaceOrientations()
            "{\"ok\":true}".withCString { done(context, $0) }
        }
        return
    }
#endif
    let response: [String: Any]
    if let options = PreviewOptions.reply(line) { response = options }
    else { switch line {
    case "launcher-state": response = ["phase": ProcessInfo.processInfo.environment["PREVIEW_MISSING_FILES"] == "1" ? "waiting" : "title", "version": "26.3"]
    case "game-files-source":
        if let url = ProcessInfo.processInfo.environment["PREVIEW_GAME_FILES_URL"] {
            response = ["url": url, "version": "26.3"]
        } else {
            response = ["error": "Automatic downloads are disabled in the preview."]
        }
    case _ where line.hasPrefix("import-game-files "):
        if ProcessInfo.processInfo.environment["PREVIEW_MISSING_FILES"] == "1",
           let bytes = try? Data(contentsOf: URL(fileURLWithPath: String(line.dropFirst("import-game-files ".count)))),
           bytes == Data("horizon-download-fixture".utf8) {
            try? FileManager.default.createDirectory(at: GameFolder.resources, withIntermediateDirectories: true)
            try? Data("{}".utf8).write(to: GameFolder.resources.appendingPathComponent("horizon-resources.json"))
            response = ["ok": true, "kind": "resources"]
        } else { response = ["error": "Unexpected preview fixture."] }
    case "worlds":
        response = ["worlds": [
            ["folder": "meadow", "name": "Meadow Valley", "mode": "survival", "version": "26.3", "last_played": Date().timeIntervalSince1970 * 1000 - 3600000],
            ["folder": "creative", "name": "Creative Workshop", "mode": "creative", "version": "26.3", "last_played": Date().timeIntervalSince1970 * 1000 - 86400000],
            ["folder": "islands", "name": "The Islands", "mode": "survival", "version": "26.3", "last_played": Date().timeIntervalSince1970 * 1000 - 172800000]
        ]]
    case "servers": response = ["servers": [["index": 0, "name": "Hypixel", "address": "mc.hypixel.net"]]]
    case "server-ping mc.hypixel.net": response = ["motd": "§6§lHypixel Network §8[§c1.8 / 26.3§8]\n§f§lSKYBLOCK §r§dHalloween Event", "online": 9813, "max": 200000, "version": "Java Edition", "ms": 79]
    default: response = ["ok": true]
    }
    }
    let data = try! JSONSerialization.data(withJSONObject: response)
    String(data: data, encoding: .utf8)!.withCString { done(context, $0) }
}

#if !ORIENTATION_FIXTURE
@main
#endif
struct LauncherPreview: App {
    @State private var previewNotice = false
    init() {
        for key in ["HORIZON_HOME", "HORIZON_LAUNCHER_HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME",
                    "HORIZON_LAUNCHER_TRASH", "HORIZON_LAUNCHER_CLIPBOARD", "HORIZON_LAUNCHER_DESKTOP"] {
            if let value = ProcessInfo.processInfo.environment[key] {
                try? FileManager.default.createDirectory(atPath: value, withIntermediateDirectories: true)
            }
        }
        if ProcessInfo.processInfo.environment["PREVIEW_MISSING_FILES"] == "1" {
            try? FileManager.default.removeItem(at: GameFolder.resources)
        }
        Theme.registerFonts()
        Bridge.submit = previewSubmit
        if ProcessInfo.processInfo.environment["PREVIEW_SIGNED_IN"] == "1" {
            let profile = MicrosoftAccount.Profile(name: "SkyExplorer", uuid: "00000000000000000000000000000000")
            UserDefaults.standard.set(try! JSONEncoder().encode(profile), forKey: "microsoftProfile")
        } else if ProcessInfo.processInfo.environment["PREVIEW_SIGNED_IN"] == "0" {
            UserDefaults.standard.removeObject(forKey: "microsoftProfile")
        }
        if ProcessInfo.processInfo.environment["PREVIEW_SIGNED_IN"] != nil {
            try? "name:OfflinePlayer\n".write(to: GameFolder.documents.appendingPathComponent("horizon-player.txt"), atomically: true, encoding: .utf8)
        }
    }
    var body: some Scene {
        WindowGroup {
            HomeView(onPlay: { previewNotice = true })
                .alert("Launcher UI preview", isPresented: $previewNotice) {
                    Button("OK", role: .cancel) {}
                } message: { Text("The worlds and server entry here are sample data for reviewing the interface. This preview does not start the game.") }
                .frame(width: ProcessInfo.processInfo.environment["PREVIEW_WIDTH"].flatMap(Double.init).map { CGFloat($0) })
        }
    }
}

#if ORIENTATION_FIXTURE
// UIKit fixture uses the production presenter, unlike the SwiftUI-only preview.
private weak var orientationFixtureRoot: OrientationFixtureGameController?

private final class OrientationFixtureAppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        Theme.registerFonts()
        Bridge.submit = previewSubmit
        UserDefaults.standard.removeObject(forKey: "microsoftProfile")
        return true
    }
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
                     options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let config = UISceneConfiguration(name: "Orientation fixture", sessionRole: session.role)
        config.delegateClass = OrientationFixtureSceneDelegate.self
        return config
    }
}
private final class OrientationFixtureSceneDelegate: NSObject, UIWindowSceneDelegate {
    var window: UIWindow?
    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let game = OrientationFixtureGameController()
        orientationFixtureRoot = game
        let window = UIWindow(windowScene: scene)
        window.rootViewController = game
        self.window = window
        window.makeKeyAndVisible()
    }
}
private final class OrientationFixtureGameController: UIViewController {
    var launcherVisible = false
    private var firstAppearance = true
    override var supportedInterfaceOrientations: UIInterfaceOrientationMask {
        launcherVisible ? .allButUpsideDown : .landscape
    }
    override var preferredInterfaceOrientationForPresentation: UIInterfaceOrientation { .landscapeRight }
    override func present(_ controller: UIViewController, animated: Bool, completion: (() -> Void)? = nil) {
        controller.view.accessibilityIdentifier = launcherVisible
            ? "launcher-presentation-permission-ready" : "launcher-presentation-before-permission"
        super.present(controller, animated: animated, completion: completion)
    }
    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        let label = UILabel()
        label.text = "Game fixture"
        label.accessibilityIdentifier = "orientation-game-root"
        label.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(label)
        NSLayoutConstraint.activate([label.centerXAnchor.constraint(equalTo: view.centerXAnchor),
                                     label.centerYAnchor.constraint(equalTo: view.centerYAnchor)])
    }
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        guard firstAppearance else { return }
        firstAppearance = false
        turn(view.window?.windowScene, to: .landscape)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            _ = horizonLauncherPresent(previewSubmit)
        }
    }
}

#endif
