import SwiftUI
import UIKit

/// Original Horizon control editor. No Amethyst/Pojav implementation is used.
/// IDs/order are the version-one ABI in touch.rs and horizon-ios-launcher.
enum TouchButton: Int, CaseIterable, Codable, Identifiable {
    case forward, back, left, right, sneak, jump, perspective, chat, pause, inventory
    var id: Int { rawValue }
    var title: String {
        switch self {
        case .forward: return "Forward"
        case .back: return "Back"
        case .left: return "Left"
        case .right: return "Right"
        case .sneak: return "Sneak"
        case .jump: return "Jump"
        case .perspective: return "View"
        case .chat: return "Chat"
        case .pause: return "Pause"
        case .inventory: return "Inventory"
        }
    }
    var symbol: String {
        switch self {
        case .forward: return "arrow.up"
        case .back: return "arrow.down"
        case .left: return "arrow.left"
        case .right: return "arrow.right"
        case .sneak: return "figure.walk"
        case .jump: return "arrow.up.to.line"
        case .perspective: return "camera.rotate"
        case .chat: return "text.bubble"
        case .pause: return "pause"
        case .inventory: return "ellipsis"
        }
    }
    // GUI pixels, matching touch::default_layout. Hotbar is the game's HUD,
    // not an independently movable touch button.
    func rect(in size: CGSize) -> CGRect {
        let w = size.width, h = size.height
        let s = min(48, max(26, h * 0.14)), small = (s * 0.65).rounded()
        let cx = 14 + s, cy = h - 14 - 2 * s - 8
        switch self {
        case .forward: return CGRect(x: cx, y: cy - s, width: s, height: s)
        case .back: return CGRect(x: cx, y: cy + s, width: s, height: s)
        case .left: return CGRect(x: cx - s, y: cy, width: s, height: s)
        case .right: return CGRect(x: cx + s, y: cy, width: s, height: s)
        case .sneak: return CGRect(x: cx, y: cy, width: s, height: s)
        case .jump: return CGRect(x: w - 14 - s, y: cy, width: s, height: s)
        case .inventory: return CGRect(x: floor(w / 2 - 91) + 184, y: h - 30, width: 30, height: 30)
        default:
            let i = rawValue - TouchButton.perspective.rawValue
            return CGRect(x: (w - 3 * small - 8) / 2 + CGFloat(i) * (small + 4), y: 2, width: small, height: small)
        }
    }
}

struct TouchCentre: Codable, Equatable {
    var x: Double
    var y: Double
    var valid: Bool { x.isFinite && y.isFinite && (0...1).contains(x) && (0...1).contains(y) }
}

struct TouchConfiguration: Codable, Equatable {
    var centres: [String: TouchCentre] = [:]
    var showControls = true

    func rect(_ button: TouchButton, in size: CGSize) -> CGRect {
        var r = button.rect(in: size)
        if let c = centres[String(button.rawValue)], c.valid {
            r.origin.x = max(0, min(size.width - r.width, c.x * size.width - r.width / 2))
            r.origin.y = max(0, min(size.height - r.height, c.y * size.height - r.height / 2))
        }
        return r
    }
    mutating func move(_ button: TouchButton, to point: CGPoint, in size: CGSize) {
        guard size.width > 0, size.height > 0 else { return }
        let r = button.rect(in: size)
        let x = max(r.width / 2, min(size.width - r.width / 2, point.x))
        let y = max(r.height / 2, min(size.height - r.height / 2, point.y))
        centres[String(button.rawValue)] = TouchCentre(x: x / size.width, y: y / size.height)
    }
    func snapshot() -> ([Float], UInt32) {
        let points = TouchButton.allCases.flatMap { b -> [Float] in
            guard let c = centres[String(b.rawValue)], c.valid else { return [.nan, .nan] }
            return [Float(c.x), Float(c.y)]
        }
        return (points, showControls ? 0 : 1)
    }
}

/// The synchronous C reader may be called from a different thread. Only plain
/// values are read under this lock; there are no UIKit calls in the render loop.
final class TouchPreferences: ObservableObject {
    static let shared = TouchPreferences()
    static let key = "touchControls.v1"
    @Published private(set) var configuration: TouchConfiguration
    private let defaults: UserDefaults
    private let lock = NSLock()
    private var cached: TouchConfiguration

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        let config = defaults.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(TouchConfiguration.self, from: $0) } ?? TouchConfiguration()
        configuration = config; cached = config
    }
    func save(_ value: TouchConfiguration) {
        guard let data = try? JSONEncoder().encode(value) else { return }
        defaults.set(data, forKey: Self.key)
        lock.lock(); cached = value; lock.unlock()
        configuration = value
    }
    func snapshot() -> ([Float], UInt32) {
        lock.lock(); defer { lock.unlock() }
        return cached.snapshot()
    }
}

@_cdecl("horizon_touch_preferences_v1")
public func horizonTouchPreferences(_ buffer: UnsafeMutablePointer<Float>?, _ capacity: UInt32) -> UInt32 {
    guard let buffer, capacity >= 20 else { return 0 }
    let (values, flags) = TouchPreferences.shared.snapshot()
    for i in 0..<20 { buffer[i] = values[i] }
    return flags
}

struct TouchControlsSettings: View {
    @ObservedObject private var preferences = TouchPreferences.shared
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            SectionHeader(title: "Touch controls")
            SecondaryButton(title: "Edit button layout", icon: "hand.draw") { presentTouchEditor() }
                .accessibilityIdentifier("touch-edit-layout")
            Toggle("Show touch controls", isOn: Binding(get: { preferences.configuration.showControls }, set: { value in
                var c = preferences.configuration; c.showControls = value; preferences.save(c)
            })).accessibilityIdentifier("touch-show-controls")
            Text("Controls remain visible during screen recording. For a clean system screenshot, turn off Show touch controls first; the touch areas remain active.")
                .font(.footnote).foregroundStyle(.secondary)
        }
    }
}

@MainActor
func presentTouchEditor() {
    guard let presenter = gameViewController() else { return }
    let host = TouchEditorController(rootView: AnyView(EmptyView()))
    host.modalPresentationStyle = .fullScreen
    let previous = presenter.view.window?.windowScene?.interfaceOrientation ?? .portrait
    let restore: UIInterfaceOrientationMask
    switch previous {
    case .landscapeLeft: restore = .landscapeLeft
    case .landscapeRight: restore = .landscapeRight
    case .portraitUpsideDown: restore = .portraitUpsideDown
    default: restore = .portrait
    }
    host.rootView = AnyView(TouchLayoutEditor(initial: TouchPreferences.shared.configuration) { [weak host] saved in
        guard let host else { return }
        if let saved { TouchPreferences.shared.save(saved) }
        let scene = host.view.window?.windowScene
        host.dismiss(animated: true) {
            turn(scene, to: UIDevice.current.userInterfaceIdiom == .pad ? restore : .portrait)
        }
    })
    presenter.present(host, animated: true)
}

private final class TouchEditorController: UIHostingController<AnyView> {
    override var supportedInterfaceOrientations: UIInterfaceOrientationMask { .landscape }
    override var preferredInterfaceOrientationForPresentation: UIInterfaceOrientation { .landscapeRight }
    override var prefersStatusBarHidden: Bool { true }
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        turn(view.window?.windowScene, to: .landscape)
    }
}

private struct TouchLayoutEditor: View {
    @State private var draft: TouchConfiguration
    @State private var dragStart: CGPoint?
    @State private var selected: TouchButton?
    @State private var confirmReset = false
    let finish: (TouchConfiguration?) -> Void
    init(initial: TouchConfiguration, finish: @escaping (TouchConfiguration?) -> Void) {
        _draft = State(initialValue: initial); self.finish = finish
    }
    @State private var toolbarCentre: CGPoint?
    @State private var toolbarStart: CGPoint?
    @State private var guiLimit = TouchEditorGeometry.savedScale()
    var body: some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            let height = geometry.size.height
            let metrics = TouchEditorGeometry(size: geometry.size, displayScale: UIScreen.main.scale, limit: guiLimit)
            let gui = metrics.gui
            let scale = metrics.pointsPerUnit
            let toolbarWidth = min(500.0, max(280.0, width - 32))
            ZStack(alignment: .topLeading) {
                Color.black
                Rectangle().stroke(Color.white.opacity(0.2), lineWidth: 1)
                    .frame(width: 182 * scale, height: 22 * scale)
                    .position(x: (gui.width / 2) * scale, y: (gui.height - 11) * scale)
                    .allowsHitTesting(false)
                    ForEach(TouchButton.allCases) { button in
                        let r = draft.rect(button, in: gui)
                        Image(systemName: button.symbol)
                            .font(.system(size: max(12, r.height * scale * 0.34), weight: .semibold))
                            .frame(width: r.width * scale, height: r.height * scale)
                            .background(selected == button ? Color.accentColor.opacity(0.45) : Color.white.opacity(0.18))
                            .overlay(Rectangle().stroke(.white.opacity(0.6)))
                            .clipShape(Rectangle())
                            .contentShape(Rectangle())
                            .position(x: r.midX * scale, y: r.midY * scale)
                            .zIndex(selected == button ? 1 : 0)
                            .accessibilityLabel(button.title)
                            .accessibilityIdentifier("touch-layout-\(button.rawValue)")
                            .gesture(DragGesture(minimumDistance: 0).onChanged { value in
                                if selected != button || dragStart == nil {
                                    selected = button; dragStart = CGPoint(x: r.midX, y: r.midY)
                                }
                                guard let start = dragStart else { return }
                                draft.move(button, to: CGPoint(x: start.x + value.translation.width / scale, y: start.y + value.translation.height / scale), in: gui)
                            }.onEnded { _ in dragStart = nil })
                    }

                VStack(spacing: 0) {
                    Image(systemName: "line.3.horizontal")
                        .frame(maxWidth: .infinity).frame(height: 28)
                        .contentShape(Rectangle())
                        .accessibilityLabel("Move editing toolbar")
                        .accessibilityIdentifier("touch-toolbar-handle")
                        .gesture(DragGesture().onChanged { value in
                            let origin = toolbarStart ?? toolbarCentre ?? CGPoint(x: width / 2, y: height / 2)
                            toolbarStart = origin
                            toolbarCentre = CGPoint(
                                x: min(width - toolbarWidth / 2, max(toolbarWidth / 2, origin.x + value.translation.width)),
                                y: min(height - 45, max(45, origin.y + value.translation.height)))
                        }.onEnded { _ in toolbarStart = nil })
            HStack(spacing: 20) {
                Button { finish(nil) } label: { Text("Cancel").frame(minWidth: 50, minHeight: 44) }
                    .accessibilityIdentifier("touch-editor-cancel")
                Spacer(minLength: 4)
                Menu {
                    ForEach(TouchButton.allCases) { button in
                        Button(button.title) { selected = button; dragStart = nil }
                    }
                } label: { Label(selected?.title ?? "Touch Layout", systemImage: "hand.draw") }
                    .accessibilityIdentifier("touch-editor-select")
                Spacer(minLength: 4)
                Button { confirmReset = true } label: { Text("Reset").frame(minWidth: 50, minHeight: 44) }
                    .accessibilityIdentifier("touch-editor-reset")
                Button { finish(draft) } label: { Text("Save").fontWeight(.semibold).frame(minWidth: 50, minHeight: 44) }
                    .accessibilityIdentifier("touch-editor-save")
            }.padding(.horizontal, 12).frame(height: 54)

                }
                .frame(width: toolbarWidth)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18))
                .position(toolbarCentre ?? CGPoint(x: width / 2, y: height / 2))
                .zIndex(2)
            }
            .frame(width: width, height: height)
            .onChange(of: geometry.size) { _ in toolbarCentre = nil; toolbarStart = nil }
        }
        .ignoresSafeArea()
        .background(Color.black)
        .foregroundStyle(.white).preferredColorScheme(.dark)
        .confirmationDialog("Reset all button positions?", isPresented: $confirmReset, titleVisibility: .visible) {
            Button("Reset Layout", role: .destructive) { draft.centres = [:]; selected = nil; dragStart = nil }
        } message: { Text("Save to keep the default positions, or Cancel to keep your current layout.") }
    }
}

/// Same integer scale selection and ceil-to-GUI dimensions as HudBatch::new.
struct TouchEditorGeometry {
    let gui: CGSize
    let pointsPerUnit: CGFloat
    init(size: CGSize, displayScale: CGFloat, limit: Int) {
        let density = max(1, displayScale)
        let pixels = (Int((size.width * density).rounded()), Int((size.height * density).rounded()))
        var scale = 1
        while (limit == 0 || scale < limit) && pixels.0 / (scale + 1) >= 320 && pixels.1 / (scale + 1) >= 240 {
            scale += 1
        }
        gui = CGSize(width: ceil(Double(pixels.0) / Double(scale)), height: ceil(Double(pixels.1) / Double(scale)))
        pointsPerUnit = CGFloat(scale) / density
    }
    static func savedScale() -> Int {
        let url = GameFolder.documents.appendingPathComponent("options.txt")
        guard let text = try? String(contentsOf: url, encoding: .utf8) else { return 0 }
        let value = text.split(separator: "\n").last(where: { $0.hasPrefix("guiScale:") })
        return max(0, value.flatMap { Int($0.dropFirst("guiScale:".count).trimmingCharacters(in: .whitespacesAndNewlines)) } ?? 0)
    }
}
