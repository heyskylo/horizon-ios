import XCTest
import UIKit
@testable import LauncherPreview

final class TouchControlsTests: XCTestCase {
    private var suite: String!
    private var defaults: UserDefaults!
    override func setUp() {
        suite = "horizon.touch.fixture." + UUID().uuidString
        defaults = UserDefaults(suiteName: suite)!
    }
    override func tearDown() { defaults.removePersistentDomain(forName: suite) }

    func testNewInstallAndResetUseDefaultButtonsVisibleInCaptures() {
        var config = TouchPreferences(defaults: defaults).configuration
        XCTAssertTrue(config.showControls)
        XCTAssertEqual(config.snapshot().1 & 1, 0)
        let size = CGSize(width: 650, height: 300)
        for button in TouchButton.allCases { XCTAssertEqual(config.rect(button, in: size), button.rect(in: size)) }
        config.move(.jump, to: CGPoint(x: 200, y: 100), in: size)
        config.centres = [:]
        XCTAssertEqual(config.rect(.jump, in: size), TouchButton.jump.rect(in: size))
    }
    func testSaveReloadClampsAndResizesTheActualButtonCentres() {
        let model = TouchPreferences(defaults: defaults)
        var draft = model.configuration
        draft.move(.jump, to: CGPoint(x: 325, y: 120), in: CGSize(width: 650, height: 300))
        XCTAssertTrue(model.configuration.centres.isEmpty, "Draft must not affect the running game")
        model.save(draft)
        let loaded = TouchPreferences(defaults: defaults).configuration
        XCTAssertEqual(loaded, draft)
        for size in [CGSize(width: 650, height: 300), CGSize(width: 1194, height: 834)] {
            let r = loaded.rect(.jump, in: size)
            XCTAssertEqual(r.midX / size.width, 0.5, accuracy: 0.00001)
            XCTAssertEqual(r.midY / size.height, 0.4, accuracy: 0.00001)
        }
        draft.move(.forward, to: CGPoint(x: -9999, y: 9999), in: CGSize(width: 650, height: 300))
        let r = draft.rect(.forward, in: CGSize(width: 650, height: 300))
        XCTAssertEqual(r.minX, 0); XCTAssertEqual(r.maxY, 300)
    }
    func testHidingPaintNeverRemovesCoordinatesAndShowingRestoresVisibility() {
        let model = TouchPreferences(defaults: defaults)
        var draft = model.configuration
        draft.move(.jump, to: CGPoint(x: 300, y: 100), in: CGSize(width: 650, height: 300))
        model.save(draft)
        let visible = model.snapshot()
        XCTAssertEqual(visible.1 & 1, 0)
        draft.showControls = false; model.save(draft)
        let hidden = model.snapshot()
        XCTAssertEqual(hidden.1 & 1, 1)
        XCTAssertEqual(hidden.0[10], visible.0[10]); XCTAssertEqual(hidden.0[11], visible.0[11])
        draft.showControls = true; model.save(draft)
        XCTAssertEqual(model.snapshot().1 & 1, 0)
    }
    func testLegacyLayoutsKeepPositionsAndGainCursorMode() throws {
        let old = Data(#"{"centres":{"5":{"x":0.7,"y":0.3}},"showControls":true}"#.utf8)
        let config = try JSONDecoder().decode(TouchConfiguration.self, from: old)
        XCTAssertEqual(config.centres["5"], TouchCentre(x:0.7,y:0.3))
        XCTAssertTrue(config.virtualMouse); XCTAssertFalse(config.joystick)
        XCTAssertEqual(config.snapshot().0.count,100)
        XCTAssertEqual(config.snapshot().1 & 2,2)
    }
    func testMalformedDataAndCoordinatesCannotMoveButtonsOffscreen() {
        defaults.set(Data("invalid".utf8), forKey: TouchPreferences.key)
        XCTAssertEqual(TouchPreferences(defaults: defaults).configuration, TouchConfiguration())
        var config = TouchConfiguration()
        for c in [TouchCentre(x: .nan, y: 0), TouchCentre(x: .infinity, y: 1), TouchCentre(x: 100, y: -5)] {
            config.centres["5"] = c
            XCTAssertEqual(config.rect(.jump, in: CGSize(width: 650, height: 300)), TouchButton.jump.rect(in: CGSize(width: 650, height: 300)))
            XCTAssertTrue(config.snapshot().0[10].isNaN)
        }
    }
    func testEditorUsesPhysicalPixelsAndGameGUIScaleWithoutAnInsetCanvas() {
        let phone = TouchEditorGeometry(size: CGSize(width: 844, height: 390), displayScale: 3, limit: 0)
        XCTAssertEqual(phone.gui, CGSize(width: 633, height: 293))
        XCTAssertEqual(phone.pointsPerUnit, 4.0 / 3.0, accuracy: 0.0001)
        let pad = TouchEditorGeometry(size: CGSize(width: 1210, height: 834), displayScale: 2, limit: 0)
        XCTAssertEqual(pad.gui, CGSize(width: 404, height: 278))
        XCTAssertEqual(pad.pointsPerUnit, 3)
        let manual = TouchEditorGeometry(size: CGSize(width: 844, height: 390), displayScale: 3, limit: 2)
        XCTAssertEqual(manual.gui, CGSize(width: 1266, height: 585))
        XCTAssertEqual(manual.pointsPerUnit, 2.0 / 3.0, accuracy: 0.0001)
    }
    func testSizingOpacityAndForwardLockRoundTrip() throws {
        let model = TouchPreferences(defaults:defaults)
        var config = model.configuration
        XCTAssertTrue(config.joystick)
        let size = CGSize(width:650,height:300)
        let original = config.rect(.joystick,in:size)
        config.resize(.joystick,factor:1.5,in:size)
        config.opacities["12"] = 0.35
        config.forwardLock = true
        model.save(config)
        let loaded = TouchPreferences(defaults:defaults).configuration
        XCTAssertEqual(loaded,config)
        let rect = loaded.rect(.joystick,in:size)
        XCTAssertEqual(rect.width,original.width*1.5,accuracy:0.0001)
        XCTAssertEqual(rect.width,rect.height)
        XCTAssertGreaterThanOrEqual(rect.minX,0)
        XCTAssertLessThanOrEqual(rect.maxY,size.height)
        let wire = loaded.snapshot()
        XCTAssertEqual(wire.0.count,100)
        XCTAssertEqual(wire.0[64],Float(rect.width / 300),accuracy:0.0001)
        XCTAssertEqual(wire.0[92],0.35,accuracy:0.0001)
        XCTAssertEqual(wire.1 & 32,32)
        for b in TouchButton.allCases { XCTAssertEqual(b.id,b.rawValue) }
        XCTAssertEqual(TouchButton.joystick.rawValue,12)
        XCTAssertEqual(TouchButton.playerList.rawValue,19)
    }
    func testV2SnapshotCapacityAndLegacyLimit() {
        var values = [Float](repeating:123,count:102)
        values.withUnsafeMutableBufferPointer { b in
            XCTAssertEqual(horizonTouchSnapshotV2(b.baseAddress,99),0)
            XCTAssertTrue(b.allSatisfy { $0 == 123 })
            _ = horizonTouchSnapshotV2(b.baseAddress,100)
        }
        XCTAssertEqual(values[100],123); XCTAssertEqual(values[101],123)
        XCTAssertEqual(horizonTouchSnapshotV2(nil,100),0)
        values = [Float](repeating:123,count:102)
        values.withUnsafeMutableBufferPointer { _ = horizonTouchPreferences($0.baseAddress,100) }
        XCTAssertTrue(values[26...].allSatisfy { $0 == 123 })
    }
    func testCExportWritesOnlyItsDocumentedCapacity() {
        var values = [Float](repeating: 123, count: 22)
        values.withUnsafeMutableBufferPointer { buffer in
            XCTAssertEqual(horizonTouchPreferences(buffer.baseAddress, 19), 0)
            XCTAssertTrue(buffer.allSatisfy { $0 == 123 })
            _ = horizonTouchPreferences(buffer.baseAddress, 20)
        }
        XCTAssertEqual(values[20], 123); XCTAssertEqual(values[21], 123)
        XCTAssertEqual(horizonTouchPreferences(nil, 20), 0)
    }
    func testHotbarSelectorUsesPhysicalIntegerCenterAndTwentyGUIRows() {
        let pixels = CGSize(width: 2420, height: 1668)
        XCTAssertEqual(TouchHotbar.slot(point: CGPoint(x: 670, y: 1547.999), pixels: pixels, scale: 6), -1)
        XCTAssertEqual(TouchHotbar.slot(point: CGPoint(x: 670, y: 1548), pixels: pixels, scale: 6), 0)
        XCTAssertEqual(TouchHotbar.slot(point: CGPoint(x: 669.999, y: 1600), pixels: pixels, scale: 6), -1)
        XCTAssertEqual(TouchHotbar.slot(point: CGPoint(x: 1749.999, y: 1600), pixels: pixels, scale: 6), 8)
        XCTAssertEqual(TouchHotbar.slot(point: CGPoint(x: 1750, y: 1600), pixels: pixels, scale: 6), -1)
        for slot in 0..<9 {
            XCTAssertEqual(TouchHotbar.slot(point: CGPoint(x: 670 + slot * 120 + 60, y: 1600), pixels: pixels, scale: 6), slot)
        }
        XCTAssertEqual(TouchHotbar.slot(point: CGPoint(x: 1210, y: 1600), pixels: pixels, scale: .nan), -1)
    }
    func testNativeGestureQueueCapacityAndContextBoundary() {
        let view = UIView(frame: CGRect(x:0,y:0,width:800,height:400))
        let bridge = TouchGestureBridge.shared
        bridge.attach(to:view)
        _ = bridge.poll(screen:9001,flags:0,width:400,height:200,scale:2,capacity:64)
        let hold = FixedGestureHold(); hold.phase = .began
        _ = bridge.perform(NSSelectorFromString("keyboardHeld:"), with:hold)
        var bytes = [Double](repeating:123,count:7)
        bytes.withUnsafeMutableBufferPointer { buffer in
            XCTAssertEqual(horizonTouchGestures(9001,0,400,200,2,buffer.baseAddress,5),0)
            XCTAssertTrue(buffer.allSatisfy{$0 == 123})
            XCTAssertEqual(horizonTouchGestures(9001,0,400,200,2,buffer.baseAddress,6),1)
        }
        XCTAssertEqual(Array(bytes.prefix(5)),[5,1,0,0,-1]);XCTAssertGreaterThanOrEqual(bytes[5],0)
        XCTAssertEqual(bytes[6],123)
        _ = bridge.perform(NSSelectorFromString("keyboardHeld:"), with:hold)
        XCTAssertTrue(bridge.poll(screen:9002,flags:0,width:400,height:200,scale:2,capacity:64).isEmpty,
                      "Old-screen gestures must never reach a new screen")
        _ = bridge.perform(NSSelectorFromString("keyboardHeld:"), with:hold)
        XCTAssertTrue(bridge.poll(screen:9002,flags:4,width:0,height:0,scale:1,capacity:64).isEmpty)
        _ = bridge.perform(NSSelectorFromString("keyboardHeld:"), with:hold)
        XCTAssertEqual(bridge.poll(screen:9002,flags:0,width:400,height:200,scale:2,capacity:64).count,1,
                       "Discarding stale input must not disable recognition for the next touch")
        _ = bridge.poll(screen:9002,flags:2,width:400,height:200,scale:2,capacity:64)
    }
    func testGestureSettingsPersistWithoutChangingOldLayoutDefaults() throws {
        let old = try JSONDecoder().decode(TouchConfiguration.self,from:Data(#"{"centres":{}}"#.utf8))
        XCTAssertTrue(old.mouseGestures);XCTAssertTrue(old.hotbarGestures)
        let model=TouchPreferences(defaults:defaults)
        var value=old;value.mouseGestures=false;value.hotbarGestures=false;model.save(value)
        let loaded=TouchPreferences(defaults:defaults).configuration
        XCTAssertFalse(loaded.mouseGestures);XCTAssertFalse(loaded.hotbarGestures)
        XCTAssertEqual(loaded.snapshot().1 & 24,24)
    }

}

private final class FixedGestureHold: UILongPressGestureRecognizer {
    var phase: UIGestureRecognizer.State = .began
    override var state: UIGestureRecognizer.State { get { phase } set { phase = newValue } }
}
