import SwiftUI
import UIKit

/// The C entry point `horizon-ios-launcher` looks up: show the launcher while the engine is stopped or starting. Play dismisses it; the game's Quit Game asks for it again. `submit` is how the
/// launcher asks the game things (Bridge.swift). Called on the main thread; the presenting waits a turn of the
/// run loop, because at startup the game's view controller may not be on screen yet while its first frame is
/// being set up.
@_cdecl("horizon_launcher_present")
public func horizonLauncherPresent(_ submit: HorizonSubmit?) -> Bool {
    Bridge.submit = submit
    Theme.registerFonts()
    DispatchQueue.main.async { present(attempts: 20) }
    return true
}

// Presentation requests can overlap while the Rust bridge acknowledges them.
private var presentationPending = false

private func present(attempts: Int) {
    guard let root = gameViewController(), root.view.window != nil else {
        // Not on screen yet: try again shortly, for a couple of seconds at most.
        if attempts > 0 {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { present(attempts: attempts - 1) }
        }
        // Do not leave the resource worker waiting forever if UIKit could not present.
        else { Task { _ = await Bridge.send("launcher-start") } }
        return
    }
    // Already up: a second request (Quit Game pressed twice) changes nothing.
    if root is LauncherHostingController || presentationPending {
        return
    }
    presentationPending = true
    let host = LauncherHostingController(rootView: AnyView(EmptyView()))
    host.rootView = AnyView(
        HomeView(onPlay: { [weak host] in
            guard let host else { return }
            GameAudio.start()
            let scene = host.view.window?.windowScene
            host.dismiss(animated: true) {
                Task { @MainActor in
                    _ = await Bridge.send("launcher-visible false")
                    turn(scene, to: .landscape)
                }
            }
        })

    )
    host.modalPresentationStyle = .fullScreen
    host.modalTransitionStyle = .crossDissolve
    Task { @MainActor in
        // iOS 16 can retain the game's landscape geometry if presentation and
        // rotation race the underlying controller's supported-mask change.
        _ = await Bridge.send("launcher-visible true")
        guard root.view.window != nil else {
            presentationPending = false
            _ = await Bridge.send("launcher-visible false")
            return
        }
        root.present(host, animated: false) {
            presentationPending = false
            turn(host.view.window?.windowScene, to: UIDevice.current.userInterfaceIdiom == .pad ? .all : .portrait)
        }
    }
}

/// Ask the scene to turn to `orientations` now, rather than when the phone is next moved.
func turn(_ scene: UIWindowScene?, to orientations: UIInterfaceOrientationMask) {
    guard let scene else { return }
    for window in scene.windows {
        window.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations()
        var top = window.rootViewController
        while let next = top?.presentedViewController {
            top = next
        }
        top?.setNeedsUpdateOfSupportedInterfaceOrientations()
    }
    scene.requestGeometryUpdate(.iOS(interfaceOrientations: orientations)) { error in
        NSLog("Horizon orientation request failed: %@", error.localizedDescription)
    }
}

/// The view controller of winit's window: the one the launcher is presented from.
func gameViewController() -> UIViewController? {
    let windows = UIApplication.shared.connectedScenes
        .filter { $0.session.role == .windowApplication }
        .compactMap { $0 as? UIWindowScene }
        .flatMap(\.windows)
    let window = windows.first(where: \.isKeyWindow) ?? windows.first
    var top = window?.rootViewController
    while let presented = top?.presentedViewController {
        top = presented
    }
    return top
}

/// The launcher stands upright, as the design does; the game under it is held sideways by its own view
/// controller after dismissal. While covered, that controller permits both orientations.
private final class LauncherHostingController: UIHostingController<AnyView> {
    override var supportedInterfaceOrientations: UIInterfaceOrientationMask {
        UIDevice.current.userInterfaceIdiom == .pad ? .all : .portrait
    }
    override var preferredInterfaceOrientationForPresentation: UIInterfaceOrientation {
        guard UIDevice.current.userInterfaceIdiom == .pad else { return .portrait }
        return UIApplication.shared.connectedScenes
            .filter { $0.session.role == .windowApplication }
        .compactMap { $0 as? UIWindowScene }
            .first(where: { $0.activationState == .foregroundActive })?.interfaceOrientation ?? .portrait
    }
    override var preferredStatusBarStyle: UIStatusBarStyle { .default }
}
