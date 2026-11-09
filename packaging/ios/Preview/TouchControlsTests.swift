import XCTest
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
        XCTAssertEqual(config.snapshot().1, 0)
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
        XCTAssertEqual(visible.1, 0)
        draft.showControls = false; model.save(draft)
        let hidden = model.snapshot()
        XCTAssertEqual(hidden.1, 1)
        XCTAssertEqual(hidden.0[10], visible.0[10]); XCTAssertEqual(hidden.0[11], visible.0[11])
        draft.showControls = true; model.save(draft)
        XCTAssertEqual(model.snapshot().1, 0)
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
}
