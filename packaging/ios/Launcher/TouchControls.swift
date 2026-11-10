import SwiftUI
import UIKit

/// Native control preferences and full-screen layout editor.
/// Input comparison and reference pins: docs/ios/pojav-input-parity.md.
/// IDs/order are the version-one ABI in touch.rs and horizon-ios-launcher.
enum TouchButton: Int, CaseIterable, Codable, Identifiable {
    case forward, back, left, right, sneak, jump, perspective, chat, pause, inventory, leftClick, rightClick, joystick, sprint, middleClick, more, hide, runtime, debug, playerList
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
        case .leftClick: return "Left click"
        case .rightClick: return "Right click"
        case .joystick: return "Joystick"
        case .sprint: return "Sprint"
        case .middleClick: return "Middle click"
        case .more: return "More"
        case .hide: return "Hide GUI"
        case .runtime: return "Runtime Menu"
        case .debug: return "Debug Menu"
        case .playerList: return "Player List"
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
        case .leftClick: return "computermouse"
        case .rightClick: return "computermouse.fill"
        case .joystick: return "circle.fill"
        case .sprint: return "figure.run"
        case .middleClick: return "cursorarrow.click"
        case .more: return "ellipsis"
        case .hide: return "eye.slash"
        case .runtime: return "line.3.horizontal"
        case .debug: return "ladybug"
        case .playerList: return "person.2"
        }
    }
    // GUI pixels, matching touch::default_layout. Hotbar is the game's HUD,
    // not an independently movable touch button.
    func rect(in size: CGSize) -> CGRect {
        let w = size.width, h = size.height
        let d = min(48, max(26, h * 0.14))
        let cx = 14 + d, cy = h - 14 - 2 * d - 16
        let unit = min(w,h) / 768, s = 69.6 * unit, stick = 204.8 * unit
        let right = 0.94878495 * w - s, gap = 2 * unit
        switch self {
        case .forward: return CGRect(x:cx,y:cy-d,width:d,height:d)
        case .back: return CGRect(x:cx,y:cy+d,width:d,height:d)
        case .left: return CGRect(x:cx-d,y:cy,width:d,height:d)
        case .right: return CGRect(x:cx+d,y:cy,width:d,height:d)
        case .sneak: return CGRect(x:right,y:0.31131014*h,width:s,height:s)
        case .jump: return CGRect(x:right,y:0.31131014*h+s+gap,width:s,height:s)
        case .sprint: return CGRect(x:right-s-gap,y:0.31131014*h,width:s,height:s)
        case .leftClick: return CGRect(x:0.7756397*w-s,y:0.8224885*h-s,width:s,height:s)
        case .rightClick: return CGRect(x:0.8756154*w-s,y:0.8178498*h-s,width:s,height:s)
        case .middleClick: return CGRect(x:0.83548373*w-s,y:0.67798775*h-s,width:s,height:s)
        case .joystick: return CGRect(x:0.02326631*w,y:min(0.9753418*h-stick,h > w ? h-34-stick : h),width:stick,height:stick)
        case .perspective: return CGRect(x:98*unit,y:h>w ? 42*unit : 0,width:96*unit,height:40*unit)
        case .chat: return CGRect(x:0,y:42*unit,width:96*unit,height:40*unit)
        case .pause: return CGRect(x:w-3*106.8*unit,y:0,width:104.8*unit,height:39.2*unit)
        case .inventory: return CGRect(x:w-3*106.8*unit-102*unit,y:0,width:100*unit,height:40*unit)
        case .more: return CGRect(x:0,y:0,width:96*unit,height:40*unit)
        case .debug: return CGRect(x:294*unit,y:h>w ? 42*unit : 0,width:96*unit,height:40*unit)
        case .playerList: return CGRect(x:196*unit,y:h>w ? 42*unit : 0,width:96*unit,height:40*unit)
        case .hide: return CGRect(x:w-69.6*unit,y:0,width:69.6*unit,height:40*unit)
        case .runtime: return CGRect(x:w-176.4*unit,y:0,width:104.8*unit,height:39.2*unit)
        }
    }
}

struct TouchCentre: Codable, Equatable {
    var x: Double
    var y: Double
    var valid: Bool { x.isFinite && y.isFinite && (0...1).contains(x) && (0...1).contains(y) }
}

struct TouchSize: Codable, Equatable {
    var width: Double
    var height: Double
    var valid: Bool { width.isFinite && height.isFinite && (0.02...0.8).contains(width) && (0.02...0.8).contains(height) }
}

struct TouchConfiguration: Codable, Equatable {
    var centres: [String: TouchCentre] = [:]
    var sizes: [String: TouchSize] = [:]
    var opacities: [String: Double] = [:]
    var forwardLock = false
    var showControls = true
    var virtualMouse = true
    var joystick = true
    var mouseGestures = true
    var hotbarGestures = true
    var slideableHotbar = false
    var holdDuration = 0.4
    enum CodingKeys: String, CodingKey { case centres, sizes, opacities, forwardLock, showControls, virtualMouse, joystick, mouseGestures, hotbarGestures, slideableHotbar, holdDuration }
    init() {}
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        centres = try c.decodeIfPresent([String: TouchCentre].self, forKey: .centres) ?? [:]
        sizes = try c.decodeIfPresent([String: TouchSize].self, forKey: .sizes) ?? [:]
        opacities = try c.decodeIfPresent([String: Double].self, forKey: .opacities) ?? [:]
        forwardLock = try c.decodeIfPresent(Bool.self, forKey: .forwardLock) ?? false
        showControls = try c.decodeIfPresent(Bool.self, forKey: .showControls) ?? true
        virtualMouse = try c.decodeIfPresent(Bool.self, forKey: .virtualMouse) ?? true
        joystick = try c.decodeIfPresent(Bool.self, forKey: .joystick) ?? false
        mouseGestures = try c.decodeIfPresent(Bool.self, forKey: .mouseGestures) ?? true
        hotbarGestures = try c.decodeIfPresent(Bool.self, forKey: .hotbarGestures) ?? true
        slideableHotbar = try c.decodeIfPresent(Bool.self, forKey: .slideableHotbar) ?? false
        let duration = try c.decodeIfPresent(Double.self, forKey: .holdDuration) ?? 0.4
        holdDuration = duration.isFinite ? min(2, max(0.1, duration)) : 0.4
    }
    var buttons: [TouchButton] {
        TouchButton.allCases.filter { button in
            if button == .joystick { return joystick }
            if [.forward, .back, .left, .right].contains(button) { return !joystick }
            return true
        }
    }

    func rect(_ button: TouchButton, in size: CGSize) -> CGRect {
        var r = button.rect(in: size)
        if let value = sizes[String(button.rawValue)], value.valid {
            let unit = min(size.width,size.height)
            let centre = CGPoint(x:r.midX,y:r.midY)
            r.size = CGSize(width:min(size.width,value.width*unit),height:min(size.height,(button == .joystick ? value.width : value.height)*unit))
            r.origin = CGPoint(x:centre.x-r.width/2,y:centre.y-r.height/2)
        }
        if let c = centres[String(button.rawValue)], c.valid {
            r.origin.x = max(0, min(size.width - r.width, c.x * size.width - r.width / 2))
            r.origin.y = max(0, min(size.height - r.height, c.y * size.height - r.height / 2))
        }
        r.origin.x = max(0,min(size.width-r.width,r.origin.x))
        r.origin.y = max(0,min(size.height-r.height,r.origin.y))
        return r
    }
    func opacity(_ button: TouchButton) -> Double {
        guard let value = opacities[String(button.rawValue)], value.isFinite else { return 1 }
        return min(1,max(0.1,value))
    }
    mutating func resize(_ button: TouchButton, factor: Double, in size: CGSize) {
        guard factor.isFinite, factor > 0, min(size.width,size.height) > 0 else { return }
        let r = rect(button,in:size), unit = min(size.width,size.height)
        sizes[String(button.rawValue)] = TouchSize(width:min(0.8,max(0.02,r.width*factor/unit)),height:min(0.8,max(0.02,r.height*factor/unit)))
        move(button,to:CGPoint(x:r.midX,y:r.midY),in:size)
    }
    mutating func move(_ button: TouchButton, to point: CGPoint, in size: CGSize) {
        guard size.width > 0, size.height > 0 else { return }
        let r = rect(button, in: size)
        let x = max(r.width / 2, min(size.width - r.width / 2, point.x))
        let y = max(r.height / 2, min(size.height - r.height / 2, point.y))
        centres[String(button.rawValue)] = TouchCentre(x: x / size.width, y: y / size.height)
    }
    func snapshot() -> ([Float], UInt32) {
        let points = TouchButton.allCases.flatMap { b -> [Float] in
            guard let c = centres[String(b.rawValue)], c.valid else { return [.nan, .nan] }
            return [Float(c.x), Float(c.y)]
        }
        let dimensions = TouchButton.allCases.flatMap { b -> [Float] in
            guard let value = sizes[String(b.rawValue)], value.valid else { return [.nan,.nan] }
            return [Float(value.width),Float(b == .joystick ? value.width : value.height)]
        }
        let alpha = TouchButton.allCases.map { Float(opacity($0)) }
        return (points + dimensions + alpha, (forwardLock ? 32 : 0) | (showControls ? 0 : 1) | (virtualMouse ? 2 : 0) | (joystick ? 4 : 0) | (mouseGestures ? 0 : 8) | (hotbarGestures ? 0 : 16))
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
    for i in 0..<min(Int(capacity), 26) { buffer[i] = values[i] }
    return flags
}

@_cdecl("horizon_touch_snapshot_v2")
public func horizonTouchSnapshotV2(_ buffer: UnsafeMutablePointer<Float>?, _ capacity: UInt32) -> UInt32 {
    guard let buffer, capacity >= 100 else { return 0 }
    let (values, flags) = TouchPreferences.shared.snapshot()
    for i in 0..<100 { buffer[i] = values[i] }
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
            Picker("Menu input", selection: Binding(get: { preferences.configuration.virtualMouse }, set: { value in
                var c = preferences.configuration; c.virtualMouse = value; preferences.save(c)
            })) { Text("Mouse cursor").tag(true); Text("Direct touch").tag(false) }
                .accessibilityIdentifier("touch-menu-mode")
            Picker("Movement", selection: Binding(get: { preferences.configuration.joystick }, set: { value in
                var c = preferences.configuration; c.joystick = value; preferences.save(c)
            })) { Text("D-pad").tag(false); Text("Joystick").tag(true) }
                .accessibilityIdentifier("touch-movement-mode")
            Toggle("Forward lock", isOn: Binding(get: { preferences.configuration.forwardLock }, set: { value in
                var c = preferences.configuration; c.forwardLock = value; preferences.save(c)
            })).disabled(!preferences.configuration.joystick)
            Toggle("Tap and hold gestures", isOn: Binding(get: { preferences.configuration.mouseGestures }, set: { value in
                var c = preferences.configuration; c.mouseGestures = value; preferences.save(c)
            }))
            Toggle("Hotbar gestures", isOn: Binding(get: { preferences.configuration.hotbarGestures }, set: { value in
                var c = preferences.configuration; c.hotbarGestures = value; preferences.save(c)
            }))
            Toggle("Slide across hotbar slots", isOn: Binding(get: { preferences.configuration.slideableHotbar }, set: { value in
                var c = preferences.configuration; c.slideableHotbar = value; preferences.save(c)
            }))
            VStack(alignment: .leading) {
                Text("Hold duration: \(Int(preferences.configuration.holdDuration * 1000)) ms")
                Slider(value: Binding(get: { preferences.configuration.holdDuration }, set: { value in
                    var c = preferences.configuration; c.holdDuration = value; preferences.save(c)
                }), in: 0.1...2, step: 0.05)
            }
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
    @State private var pinchScale: CGFloat = 1
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
                    ForEach(draft.buttons) { button in
                        let r = draft.rect(button, in: gui)
                        Text(button == .joystick ? "" : button.title)
                            .multilineTextAlignment(.center)
                            .font(.system(size: max(12, r.height * scale * 0.28), weight: .semibold))
                            .opacity(button == .joystick ? 0 : 1)
                            .frame(width: r.width * scale, height: r.height * scale)
                            .background {
                                if button == .joystick { Circle().fill(selected == button ? Color.accentColor.opacity(0.3) : Color.white.opacity(0.18)).frame(width:r.width * scale * 0.75,height:r.height * scale * 0.75) }
                                else { RoundedRectangle(cornerRadius: r.height * scale * 0.2).fill(selected == button ? Color.accentColor.opacity(0.45) : Color.white.opacity(0.18)) }
                            }
                            .overlay {
                                if button == .joystick {
                                    if selected == button { Circle().stroke(.white.opacity(0.6)).frame(width:r.width * scale * 0.75,height:r.height * scale * 0.75) }
                                    Circle().fill(.black)
                                        .frame(width: min(r.width,r.height) * scale * 0.25,
                                               height: min(r.width,r.height) * scale * 0.25)
                                }
                                else { RoundedRectangle(cornerRadius:r.height * scale * 0.2).stroke(.white.opacity(0.6)) }
                            }
                            .opacity(draft.opacity(button))
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
                            .simultaneousGesture(MagnificationGesture().onChanged { value in
                                selected = button
                                draft.resize(button,factor:value / pinchScale,in:gui)
                                pinchScale = value
                            }.onEnded { _ in pinchScale = 1 })
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
                                y: min(height - 80, max(80, origin.y + value.translation.height)))
                        }.onEnded { _ in toolbarStart = nil })
            HStack(spacing: 20) {
                Button { finish(nil) } label: { Text("Cancel").frame(minWidth: 50, minHeight: 44) }
                    .accessibilityIdentifier("touch-editor-cancel")
                Spacer(minLength: 4)
                Menu {
                    ForEach(draft.buttons) { button in
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
                    if let selected {
                        HStack {
                            Button("Size −") { draft.resize(selected,factor:0.9,in:gui) }
                            Button("Size +") { draft.resize(selected,factor:1.1,in:gui) }
                            Text("Opacity")
                            Slider(value:Binding(get:{ draft.opacity(selected) },set:{ draft.opacities[String(selected.rawValue)] = $0 }),in:0.1...1)
                        }.padding(.horizontal,16).padding(.bottom,12)
                    }

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
            Button("Reset Layout", role: .destructive) { draft.centres = [:]; draft.sizes = [:]; draft.opacities = [:]; draft.forwardLock = false; selected = nil; dragStart = nil }
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

/// UIKit recognizes the same tap/long-press/two-finger gestures used by the
/// reference launcher. The Rust frontend consumes their results on its frame
/// thread; the callbacks never access game state or player files.
final class TouchGestureBridge: NSObject, UIGestureRecognizerDelegate {
    static let shared = TouchGestureBridge()
    private weak var surface: UIView?
    private var recognizers: [UIGestureRecognizer] = []
    private var screen: UInt64 = 0
    private var active = false
    private var playing = false
    private var gui = CGSize.zero
    private var guiScale: Double = 1
    private var pending: [(Double, [Double], UInt64, UInt32, UInt64)] = []
    var inputContext: UInt64 = 0
    private var primaryTouch: UITouch?
    private var hotbarTouch: UITouch?
    private var hotbarHold: UILongPressGestureRecognizer?
    private var hotbarSlot = -1
    private var suppressTap = false

    func attach(to view: UIView) {
        guard surface !== view else { return }
        if let surface { for recognizer in recognizers { surface.removeGestureRecognizer(recognizer) } }
        surface = view
        let tap = TouchSurfaceTap(target: self, action: #selector(tapped(_:)))
        let double = UITapGestureRecognizer(target: self, action: #selector(doubleTapped(_:)))
        double.numberOfTapsRequired = 2
        let hold = UILongPressGestureRecognizer(target: self, action: #selector(held(_:)))
        hold.minimumPressDuration = TouchPreferences.shared.configuration.holdDuration
        hotbarHold = hold
        let keyboard = UILongPressGestureRecognizer(target: self, action: #selector(keyboardHeld(_:)))
        keyboard.numberOfTouchesRequired = 2
        let scroll = UIPanGestureRecognizer(target: self, action: #selector(scrolled(_:)))
        scroll.minimumNumberOfTouches = 2; scroll.maximumNumberOfTouches = 2
        let hotbar = TouchHotbarTracker(target: self, action: #selector(hotbarMoved(_:)))
        recognizers = [tap, double, hold, keyboard, scroll, hotbar]
        for recognizer in recognizers {
            recognizer.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]
            recognizer.cancelsTouchesInView = false
            recognizer.delegate = self
            view.addGestureRecognizer(recognizer)
        }
        scroll.cancelsTouchesInView = true
    }

    func poll(screen: UInt64, flags: UInt32, width: Double, height: Double, scale: Double, capacity: Int, withContext: Bool = false) -> [[Double]] {
        if flags & 4 != 0 { pending.removeAll(); return [] }
        hotbarHold?.minimumPressDuration = TouchPreferences.shared.configuration.holdDuration
        let enabled = flags & 2 == 0
        if self.screen != screen || active != enabled { pending.removeAll(); hotbarSlot = -1; suppressTap = false }
        self.screen = screen; active = enabled; playing = flags & 1 != 0
        gui = CGSize(width: width, height: height); guiScale = max(1, scale)
        let now = ProcessInfo.processInfo.systemUptime
        let count = min(max(0, capacity), pending.count)
        let output = pending.prefix(count).map { timestamp, values, context, modifiers, touch in
            values + [max(0, now - timestamp)] + (withContext
                ? [Double(context & 0xffff_ffff), Double(context >> 32), Double(modifiers),
                   Double(touch & 0xffff_ffff), Double(touch >> 32)] : [])
        }
        pending.removeFirst(count)
        return output
    }

    private func point(_ location: CGPoint) -> CGPoint {
        guard let surface, surface.bounds.width > 0, surface.bounds.height > 0 else { return .zero }
        let factor = Double(surface.window?.screen.scale ?? UIScreen.main.scale) / guiScale
        return CGPoint(x: location.x * factor, y: location.y * factor)
    }
    private func slot(_ location: CGPoint) -> Int {
        guard playing, TouchPreferences.shared.configuration.hotbarGestures else { return -1 }
        guard let surface else { return -1 }
        let displayScale = Double(surface.window?.screen.scale ?? UIScreen.main.scale)
        return TouchHotbar.slot(point: CGPoint(x: location.x * displayScale, y: location.y * displayScale),
                                pixels: CGSize(width: surface.bounds.width * displayScale,
                                               height: surface.bounds.height * displayScale), scale: guiScale)
    }
    private func emit(_ kind: Double, phase: Int, location: CGPoint = .zero, slot: Int = -1,
                      modifiers: UIKeyModifierFlags = [], touch: UInt64 = 0) {
        guard active else { return }
        // If an event storm exhausts the bounded queue, cancel the old gesture
        // state rather than lose an up edge and leave a button held forever.
        if pending.count >= 128 { pending = [(ProcessInfo.processInfo.systemUptime, [0, 4, 0, 0, -1], inputContext, 0, 0)] }
        let bits: UInt32 = (modifiers.contains(.shift) ? 1 : 0) | (modifiers.contains(.control) ? 2 : 0)
            | (modifiers.contains(.alternate) ? 4 : 0) | (modifiers.contains(.command) ? 8 : 0)
        pending.append((ProcessInfo.processInfo.systemUptime, [kind, Double(phase), location.x, location.y, Double(slot)], inputContext, bits, touch))
    }
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        guard active, gui.width > 0, gui.height > 0, let surface,
              surface.window?.rootViewController?.presentedViewController == nil else { return false }
        let location = touch.location(in: surface), p = point(location)
        let configuration = TouchPreferences.shared.configuration
        if configuration.buttons.contains(where: { configuration.rect($0, in: gui).contains(p) }) { return false }
        // The existing close/keyboard affordances own their touches.
        if p.y < 38 && p.x > gui.width - 76 { return false }
        hotbarSlot = slot(location)
        suppressTap = false
        if gestureRecognizer is TouchHotbarTracker { return playing && hotbarSlot >= 0 }
        if gestureRecognizer is TouchSurfaceTap || gestureRecognizer is UILongPressGestureRecognizer {
            return configuration.mouseGestures || hotbarSlot >= 0 || (gestureRecognizer as? UILongPressGestureRecognizer)?.numberOfTouchesRequired == 2
        }
        if let tap = gestureRecognizer as? UITapGestureRecognizer, tap.numberOfTapsRequired == 2 {
            return configuration.hotbarGestures
        }
        return configuration.mouseGestures
    }
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer,
                           shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer) -> Bool { true }
    @objc private func hotbarMoved(_ sender: TouchHotbarTracker) {
        guard TouchPreferences.shared.configuration.slideableHotbar, playing else { return }
        let next = slot(sender.location(in: surface))
        if next >= 0 && (sender.state == .began || next != sender.lastSlot) {
            emit(6, phase: sender.state.rawValue, slot: next, modifiers: sender.modifierFlags)
            sender.lastSlot = next
        }
    }
    @objc private func tapped(_ sender: TouchSurfaceTap) {
        guard sender.state == .recognized, sender.clickEligible, !suppressTap else { return }
        emit(1, phase: sender.state.rawValue, location: point(sender.location(in: surface)), slot: hotbarSlot,
             modifiers: sender.modifierFlags, touch: sender.touchIdentity)
    }
    @objc private func doubleTapped(_ sender: UITapGestureRecognizer) {
        guard sender.state == .recognized, playing, let surface else { return }
        let current = slot(sender.location(in: surface))
        if current >= 0 && current == hotbarSlot { emit(3, phase: sender.state.rawValue, slot: current, modifiers: sender.modifierFlags) }
    }
    @objc private func held(_ sender: UILongPressGestureRecognizer) {
        guard let surface else { return }
        if sender.state == .began { suppressTap = true }
        emit(2, phase: sender.state.rawValue, slot: slot(sender.location(in: surface)), modifiers: sender.modifierFlags)
    }
    @objc private func keyboardHeld(_ sender: UILongPressGestureRecognizer) {
        if sender.state == .began { emit(5, phase: sender.state.rawValue, modifiers: sender.modifierFlags) }
    }
    @objc private func scrolled(_ sender: UIPanGestureRecognizer) {
        guard !playing, let surface, [.began, .changed, .ended].contains(sender.state) else { return }
        let velocity = sender.velocity(in: surface)
        guard surface.bounds.width > 0, surface.bounds.height > 0 else { return }
        emit(4, phase: sender.state.rawValue,
             location: CGPoint(x: velocity.x / surface.bounds.width, y: velocity.y / surface.bounds.height))
    }
}

/// Preserve the reference launcher's 5-by-5-point click eligibility rectangle
/// in addition to UIKit's own tap recognition. Moving out and back is a drag.
final class TouchSurfaceTap: UITapGestureRecognizer {
    private var clickRange = CGRect.zero
    private(set) var clickEligible = true
    private(set) var touchIdentity: UInt64 = 0
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        if let touch = touches.first {
            touchIdentity = UInt64(UInt(bitPattern: Unmanaged.passUnretained(touch).toOpaque()))
            let p = touch.location(in: view)
            clickRange = CGRect(x: p.x - 2, y: p.y - 2, width: 5, height: 5)
            clickEligible = true
        }
        super.touchesBegan(touches, with: event)
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        if let touch = touches.first, !clickRange.contains(touch.location(in: view)) { clickEligible = false }
        super.touchesMoved(touches, with: event)
    }
}

/// Six doubles per recognized input: kind, UIKit phase, x, y, hotbar slot,
/// and age in seconds. Called on UIKit's main thread by the game frame loop.
@_cdecl("horizon_touch_gestures_v1")
public func horizonTouchGestures(_ screen: UInt64, _ flags: UInt32, _ width: Double, _ height: Double,
                                 _ scale: Double, _ buffer: UnsafeMutablePointer<Double>?, _ capacity: UInt32) -> UInt32 {
    guard let buffer, capacity >= 6 else { return 0 }
    let events = TouchGestureBridge.shared.poll(screen: screen, flags: flags, width: width, height: height, scale: scale,
                                               capacity: Int(capacity / 6))
    for (index, values) in events.enumerated() {
        for component in 0..<6 { buffer[index * 6 + component] = values[component] }
    }
    return UInt32(events.count)
}

/// The reference bridge's physical-pixel hotbar selector (GUI scale applied
/// before integer centering), shared by native gesture hit tests.
enum TouchHotbar {
    static func slot(point: CGPoint, pixels: CGSize, scale: Double) -> Int {
        guard scale.isFinite, scale > 0, pixels.width.isFinite, pixels.height.isFinite,
              point.x.isFinite, point.y.isFinite, pixels.width >= 180 * scale,
              pixels.height >= 20 * scale else { return -1 }
        let width = Int(180 * scale), height = Int(20 * scale)
        let left = Int(pixels.width) / 2 - width / 2
        guard point.y >= Double(Int(pixels.height) - height),
              point.x >= Double(left), point.x < Double(left + width) else { return -1 }
        return min(8, Int((point.x - Double(left)) / Double(width) * 9))
    }
}

@_cdecl("horizon_touch_set_context_v1")
public func horizonTouchSetContext(_ context: UInt64) { TouchGestureBridge.shared.inputContext = context }

/// Version two appends context-id low/high words, hardware modifiers and touch-id
/// low/high words. Version one remains available for older bundled readers.
@_cdecl("horizon_touch_gestures_v2")
public func horizonTouchGesturesV2(_ screen: UInt64, _ flags: UInt32, _ width: Double, _ height: Double,
                                  _ scale: Double, _ buffer: UnsafeMutablePointer<Double>?, _ capacity: UInt32) -> UInt32 {
    guard let buffer, capacity >= 11 else { return 0 }
    let events = TouchGestureBridge.shared.poll(screen: screen, flags: flags, width: width, height: height,
                                               scale: scale, capacity: Int(capacity / 11), withContext: true)
    for (index, values) in events.enumerated() {
        for component in 0..<11 { buffer[index * 11 + component] = values[component] }
    }
    return UInt32(events.count)
}

/// A hotbar-only observer; it never cancels the game view's raw touch stream.
final class TouchHotbarTracker: UIGestureRecognizer {
    var lastSlot = -1
    private weak var finger: UITouch?
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        guard finger == nil, let first = touches.first else { return }
        finger = first; lastSlot = -1; state = .began
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        if let finger, touches.contains(finger) { state = .changed }
    }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        if let finger, touches.contains(finger) { state = .ended }
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { state = .cancelled }
    override func reset() { finger = nil; lastSlot = -1; super.reset() }
    override func location(in view: UIView?) -> CGPoint { finger?.location(in: view) ?? .zero }
}
