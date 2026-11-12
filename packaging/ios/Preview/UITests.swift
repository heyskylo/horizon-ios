import XCTest

final class LauncherLayoutTests: XCTestCase {
    private func launch(signedIn: Bool, optionsResponse: String? = nil, gameFilesURL: String? = nil, holdImport: Bool = false) -> XCUIApplication {
        let app = XCUIApplication()
        let root = NSTemporaryDirectory() + "horizon-native-" + UUID().uuidString
        app.launchEnvironment = ["PREVIEW_SIGNED_IN": signedIn ? "1" : "0",
            "HORIZON_HOME": root + "/game", "HORIZON_LAUNCHER_HOME": root + "/launcher",
            "XDG_DATA_HOME": root + "/data", "XDG_CONFIG_HOME": root + "/config",
            "HORIZON_LAUNCHER_TRASH": root + "/trash", "HORIZON_LAUNCHER_CLIPBOARD": root + "/clipboard",
            "HORIZON_LAUNCHER_DESKTOP": root + "/desktop", "HORIZON_LAUNCHER_NO_SELF_UPDATE": "1", "HORIZON_NO_AUDIO": "1"]
        if let optionsResponse { app.launchEnvironment["PREVIEW_OPTIONS_RESPONSE"] = optionsResponse }
        if let gameFilesURL {
            app.launchEnvironment["PREVIEW_MISSING_FILES"] = "1"
            app.launchEnvironment["PREVIEW_GAME_FILES_URL"] = gameFilesURL
        }
        if holdImport { app.launchEnvironment["PREVIEW_HOLD_IMPORT"] = "1" }
        app.launch()
        XCTAssertTrue(app.buttons["launcher-play-button"].waitForExistence(timeout: 15))
        return app
    }
    func testLogsExportPresentsFileSavePicker() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = launch(signedIn:false)
        app.buttons.matching(NSPredicate(format:"label == %@","Settings")).firstMatch.tap()
        let export = app.buttons["export-diagnostics"]
        XCTAssertTrue(export.waitForExistence(timeout:10))
        for _ in 0..<5 {if export.isHittable {break};app.swipeUp()}
        export.tap()
        XCTAssertTrue(app.otherElements["diagnostic-file-export"].waitForExistence(timeout:15), app.debugDescription)
        app.buttons["Cancel"].firstMatch.tap()
        XCTAssertTrue(export.waitForExistence(timeout:5))
    }
    func testNativeTapHoldAndHotbarRecognizersReachTheCBridge() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = XCUIApplication()
        let root = NSTemporaryDirectory() + "horizon-native-gesture-" + UUID().uuidString
        app.launchEnvironment = ["PREVIEW_NATIVE_GESTURES":"world",
            "HORIZON_HOME":root+"/game", "HORIZON_LAUNCHER_HOME":root+"/launcher",
            "XDG_DATA_HOME":root+"/data", "XDG_CONFIG_HOME":root+"/config",
            "HORIZON_LAUNCHER_NO_SELF_UPDATE":"1", "HORIZON_NO_AUDIO":"1"]
        app.launch()
        let canvas = app.otherElements["native-gesture-canvas"]
        let events = app.staticTexts["native-gesture-events"]
        XCTAssertTrue(events.waitForExistence(timeout: 10))
        expectation(for:NSPredicate { _,_ in canvas.frame.width > canvas.frame.height },evaluatedWith:canvas)
        waitForExpectations(timeout:5)
        let empty = canvas.coordinate(withNormalizedOffset: CGVector(dx:0.6,dy:0.5))
        empty.tap()
        expectation(for:NSPredicate(format:"label CONTAINS %@","1:3:-1"),evaluatedWith:events)
        waitForExpectations(timeout:5)
        empty.press(forDuration:0.6)
        expectation(for:NSPredicate(format:"label CONTAINS %@","2:1:-1"),evaluatedWith:events)
        waitForExpectations(timeout:5)
        XCTAssertTrue(events.label.contains("2:3:-1"))
        let hotbar = app.staticTexts["native-gesture-hotbar"]
        hotbar.coordinate(withNormalizedOffset:CGVector(dx:0.5,dy:0.5)).doubleTap()
        expectation(for:NSPredicate(format:"label CONTAINS %@","3:3:4"),evaluatedWith:events)
        waitForExpectations(timeout:5)
        hotbar.coordinate(withNormalizedOffset:CGVector(dx:0.5,dy:0.5)).press(forDuration:0.6)
        expectation(for:NSPredicate(format:"label CONTAINS %@","2:1:4"),evaluatedWith:events)
        waitForExpectations(timeout:5)
        XCTAssertTrue(events.label.contains("2:3:4"))
    }

    func testAddBindAndReloadCustomButton() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app=launch(signedIn:false)
        app.buttons.matching(NSPredicate(format:"label == %@","Settings")).firstMatch.tap()
        let edit=app.buttons["touch-edit-layout"]
        XCTAssertTrue(edit.waitForExistence(timeout:5));edit.tap()
        XCTAssertTrue(app.buttons["touch-editor-select"].waitForExistence(timeout:5))
        app.buttons["touch-editor-select"].tap();app.buttons["Add custom button"].tap()
        let name=app.textFields["custom-button-name"]
        XCTAssertTrue(name.waitForExistence(timeout:5));name.tap()
        let old=(name.value as? String) ?? ""
        name.typeText(String(repeating:XCUIKeyboardKey.delete.rawValue,count:old.count)+"My action")
        app.buttons["custom-binding-0"].tap();app.buttons["Left mouse"].firstMatch.tap()
        app.buttons["Done"].firstMatch.tap()
        app.buttons["touch-editor-save"].tap()
        XCTAssertTrue(edit.waitForExistence(timeout:5));edit.tap()
        app.buttons["touch-editor-select"].tap();app.buttons["My action"].firstMatch.tap()
        XCTAssertTrue(name.waitForExistence(timeout:5));XCTAssertEqual(name.value as? String,"My action")
        XCTAssertTrue(app.buttons["custom-binding-0"].label.contains("Left mouse"))
        app.buttons["Done"].firstMatch.tap();app.buttons["touch-editor-cancel"].tap()
    }

    func testCustomButtonChordUsesNativePressAndRelease() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app=XCUIApplication()
        let root=NSTemporaryDirectory()+"horizon-custom-"+UUID().uuidString
        app.launchEnvironment=["PREVIEW_NATIVE_GESTURES":"world","PREVIEW_CUSTOM_CONTROLS":"1",
            "HORIZON_HOME":root+"/game","HORIZON_LAUNCHER_HOME":root+"/launcher","XDG_DATA_HOME":root+"/data","XDG_CONFIG_HOME":root+"/config","HORIZON_NO_AUDIO":"1","HORIZON_LAUNCHER_NO_SELF_UPDATE":"1"]
        app.launch()
        let button=app.buttons["Custom chord"]
        XCTAssertTrue(button.waitForExistence(timeout:10));button.press(forDuration:0.2)
        let output=app.staticTexts["native-gesture-events"]
        expectation(for:NSPredicate(format:"label CONTAINS %@","7:3:-1:86"),evaluatedWith:output);waitForExpectations(timeout:5)
        for edge in ["7:1:-1:341","7:1:-1:86","7:3:-1:341","7:3:-1:86"] {XCTAssertTrue(output.label.contains(edge))}
        XCTAssertFalse(output.label.contains("1:3:-1"),"Custom controls must not also generate surface clicks")
    }

    func testTouchEditorLandscapeSaveCancelResetAndPortraitReturn() {
        XCUIDevice.shared.orientation = .portrait
        let app = launch(signedIn: false)
        app.tabBars.buttons["Settings"].tap()
        let edit = app.buttons["touch-edit-layout"]
        XCTAssertTrue(edit.waitForExistence(timeout: 5)); edit.tap()
        let save = app.buttons["touch-editor-save"]
        XCTAssertTrue(save.waitForExistence(timeout: 5))
        expectation(for: NSPredicate { _, _ in
            let frame = app.windows.firstMatch.frame
            return frame.width > frame.height
        }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let jump = app.images["touch-layout-5"]
        XCTAssertTrue(jump.waitForExistence(timeout: 5))
        app.buttons["touch-editor-reset"].tap()
        app.buttons["Reset Layout"].tap()
        let original = jump.frame
        let editorShot = XCTAttachment(screenshot: app.screenshot()); editorShot.name = "Landscape touch editor"; editorShot.lifetime = .keepAlways; add(editorShot)
        let target = app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.72, dy: 0.72))
        jump.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.1, thenDragTo: target)
        XCTAssertEqual(jump.frame.midX, app.windows.firstMatch.frame.width * 0.72, accuracy: 10)
        save.tap()
        XCTAssertTrue(edit.waitForExistence(timeout: 5))
        expectation(for: NSPredicate { _, _ in
            let frame = app.windows.firstMatch.frame
            return frame.height > frame.width
        }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        edit.tap(); XCTAssertTrue(save.waitForExistence(timeout: 5))
        let stored = jump.frame
        XCTAssertGreaterThan(abs(stored.midX - original.midX), 40)
        jump.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.1, thenDragTo: app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.5)))
        app.buttons["touch-editor-cancel"].tap()
        XCTAssertTrue(edit.waitForExistence(timeout: 5)); edit.tap()
        XCTAssertTrue(save.waitForExistence(timeout: 5))
        XCTAssertEqual(jump.frame.midX, stored.midX, accuracy: 5)
        app.buttons["touch-editor-reset"].tap()
        app.buttons["Reset Layout"].tap()
        XCTAssertEqual(jump.frame.midX, original.midX, accuracy: 5)
        save.tap()
        XCTAssertTrue(edit.waitForExistence(timeout: 5))
        let shot = XCTAttachment(screenshot: app.screenshot()); shot.name = "Touch settings"; shot.lifetime = .keepAlways; add(shot)
    }

    func testMouseButtonsAndOptionalJoystickAreEditable() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = launch(signedIn: false)
        app.buttons.matching(NSPredicate(format: "label == %@", "Settings")).firstMatch.tap()
        let mode = app.buttons["touch-movement-mode"]
        XCTAssertTrue(mode.waitForExistence(timeout: 5))
        mode.tap(); app.buttons["Joystick"].firstMatch.tap()
        app.buttons["touch-edit-layout"].tap()
        XCTAssertTrue(app.buttons["touch-editor-save"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.images["touch-layout-10"].exists)
        XCTAssertTrue(app.images["touch-layout-11"].exists)
        XCTAssertTrue(app.images["touch-layout-12"].exists)
        XCTAssertFalse(app.images["touch-layout-0"].exists)
        app.buttons["touch-editor-save"].tap()
        XCTAssertTrue(mode.waitForExistence(timeout: 5))
        mode.tap(); app.buttons["D-pad"].firstMatch.tap()
    }

    func testTabletTouchEditorKeepsToolbarOutsideCanvasAndRestoresOrientation() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = launch(signedIn: false)
        let settings = app.buttons.matching(NSPredicate(format: "label == %@", "Settings")).firstMatch
        XCTAssertTrue(settings.waitForExistence(timeout: 5)); settings.tap()
        let edit = app.buttons["touch-edit-layout"]
        XCTAssertTrue(edit.waitForExistence(timeout: 5)); edit.tap()
        let save = app.buttons["touch-editor-save"]
        XCTAssertTrue(save.waitForExistence(timeout: 5))
        app.buttons["touch-editor-reset"].tap(); app.buttons["Reset Layout"].tap()
        let jump = app.images["touch-layout-5"]
        XCTAssertTrue(jump.waitForExistence(timeout: 5))
        let original = jump.frame
        let oldSave = save.frame
        let handle = app.images["touch-toolbar-handle"]
        XCTAssertTrue(handle.isHittable)
        handle.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.1,
            thenDragTo: app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.45)))
        XCTAssertGreaterThan(abs(save.frame.midX - oldSave.midX), 40)
        XCTAssertEqual(jump.frame, original, "Moving toolbar must not resize or reposition game controls")
        jump.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.1,
            thenDragTo: app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.75, dy: 0.72)))
        let moved = jump.frame
        XCTAssertGreaterThan(abs(moved.midX - original.midX), 40)
        XCTAssertGreaterThan(moved.minY, save.frame.maxY)
        let shot = XCTAttachment(screenshot: app.screenshot()); shot.name = "Tablet landscape touch editor"; shot.lifetime = .keepAlways; add(shot)
        save.tap()
        XCTAssertTrue(edit.waitForExistence(timeout: 5))
        XCTAssertGreaterThan(app.windows.firstMatch.frame.width, app.windows.firstMatch.frame.height)
        edit.tap(); XCTAssertTrue(save.waitForExistence(timeout: 5))
        XCTAssertEqual(jump.frame.midX, moved.midX, accuracy: 5)
        XCTAssertEqual(jump.frame.midY, moved.midY, accuracy: 5)
        app.buttons["touch-editor-reset"].tap(); app.buttons["Reset Layout"].tap(); save.tap()
    }

    func testFirstRunAutomaticallyDownloadsAndEnablesPlay() throws {
        guard let url = ProcessInfo.processInfo.environment["HORIZON_DOWNLOAD_FIXTURE_URL"] else {
            throw XCTSkip("Requires the loopback download fixture")
        }
        let app = launch(signedIn: false, gameFilesURL: url)
        let play = app.buttons["launcher-play-button"]
        XCTAssertTrue(play.waitForExistence(timeout: 15))
        let ready = NSPredicate(format: "enabled == true AND label == 'Play'")
        expectation(for: ready, evaluatedWith: play)
        waitForExpectations(timeout: 20)
        XCTAssertFalse(app.staticTexts["Game files needed"].exists)
        XCTAssertFalse(app.alerts["Launcher UI preview"].exists, "Setup must not launch the game")
    }

    func testImportCommitKeepsPlayDisabledUntilAcknowledged() throws {
        guard let url = ProcessInfo.processInfo.environment["HORIZON_DOWNLOAD_FIXTURE_URL"] else {
            throw XCTSkip("Requires the loopback download fixture")
        }
        let app = launch(signedIn: false, gameFilesURL: url, holdImport: true)
        let play = app.buttons["launcher-play-button"]
        // The preview has placed the marker, but the importer has not yet
        // acknowledged its transaction. File presence alone cannot enable Play.
        expectation(for: NSPredicate(format: "label == 'Play'"), evaluatedWith: play)
        waitForExpectations(timeout: 15)
        XCTAssertFalse(play.isEnabled)
        let finish = app.buttons["fixture-finish-import"]
        XCTAssertTrue(finish.isHittable)
        finish.tap()
        expectation(for: NSPredicate(format: "enabled == true"), evaluatedWith: play)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.alerts["Launcher UI preview"].exists)
    }

    func testPreviewPlayUsesOwnedStartupAndPresentationCommit() {
        let app = launch(signedIn: false)
        let play = app.buttons["launcher-play-button"]
        XCTAssertTrue(play.isEnabled)
        play.tap()
        XCTAssertTrue(app.alerts["Launcher UI preview"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.alerts["Something went wrong"].exists)
    }

    func testFirstRunDownloadFailureKeepsRetryAndManualFallbackAvailable() throws {
        guard let url = ProcessInfo.processInfo.environment["HORIZON_DOWNLOAD_FIXTURE_URL"] else {
            throw XCTSkip("Requires the loopback download fixture")
        }
        let app = launch(signedIn: false, gameFilesURL: url + "?fail=1")
        let failure = app.staticTexts.matching(NSPredicate(format: "label CONTAINS '503'")).firstMatch
        XCTAssertTrue(failure.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["launcher-play-button"].isEnabled)
        XCTAssertTrue(app.buttons["launcher-download-game-files"].isEnabled)
        app.buttons["Use a download link instead"].tap()
        XCTAssertTrue(app.staticTexts["Link"].firstMatch.waitForExistence(timeout: 5))
    }

    func testOversizedAutomaticArchiveStopsBeforeImport() throws {
        guard let url = ProcessInfo.processInfo.environment["HORIZON_DOWNLOAD_FIXTURE_URL"] else {
            throw XCTSkip("Requires the loopback download fixture")
        }
        let app = launch(signedIn: false, gameFilesURL: url + "?oversized=1")
        let failure = app.staticTexts.matching(NSPredicate(format: "label CONTAINS '2 GiB'")).firstMatch
        XCTAssertTrue(failure.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["launcher-play-button"].isEnabled)
        XCTAssertTrue(app.buttons["launcher-download-game-files"].isEnabled)
    }

    func testPhoneLayoutAndSignedInIdentity() {
        XCUIDevice.shared.orientation = .portrait
        let app = launch(signedIn: true)
        XCTAssertFalse(app.staticTexts["Jump back in"].exists)
        XCTAssertFalse(app.staticTexts["Meadow Valley"].exists)
        XCTAssertFalse(app.buttons["launcher-nav-worlds"].exists)
        XCTAssertFalse(app.buttons["launcher-nav-servers"].exists)
        let play = app.buttons["launcher-play-button"]
        XCTAssertTrue(play.isHittable)
        XCTAssertLessThanOrEqual(play.frame.maxX, app.frame.maxX)
        XCTAssertGreaterThanOrEqual(play.frame.minX, 0)
        XCTAssertGreaterThanOrEqual(play.frame.height, 44)
        app.buttons["launcher-account-button"].tap()
        XCTAssertTrue(app.staticTexts["Microsoft account"].firstMatch.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["Change Offline Name"].exists)
        app.buttons["Done"].tap()
        let shot = XCTAttachment(screenshot: app.screenshot()); shot.name = "Native-launcher"; shot.lifetime = .keepAlways; add(shot)
    }
    func testSavedMicrosoftAccountsCanSwitchWithoutEditingTheirNames() {
        let app = launch(signedIn: true)
        app.buttons["launcher-account-button"].tap()
        let second = app.buttons["account-select-22222222222222222222222222222222"]
        XCTAssertTrue(second.waitForExistence(timeout: 5)); second.tap()
        XCTAssertFalse(app.buttons["Change Offline Name"].exists)
        XCTAssertTrue(app.staticTexts["BuilderTwo"].firstMatch.exists)
        app.buttons["Done"].tap()
        XCTAssertTrue(app.staticTexts["BuilderTwo"].firstMatch.waitForExistence(timeout: 5))
    }

    func testOfflineNameEditing() {
        let app = launch(signedIn: false)
        app.buttons["launcher-account-button"].tap()
        let edit = app.buttons["Change Offline Name"]
        let field = app.textFields["offline-player-name"]
        func openEditor() {
            XCTAssertTrue(edit.waitForExistence(timeout: 5)); edit.tap()
            XCTAssertTrue(field.waitForExistence(timeout: 5))
        }
        openEditor()
        field.doubleTap(); field.typeText("TrailRider")
        XCTAssertEqual(field.value as? String, "TrailRider")
        app.buttons["Save"].tap()
        XCTAssertTrue(app.staticTexts["TrailRider"].firstMatch.waitForExistence(timeout: 5))

        openEditor()
        XCTAssertEqual(field.value as? String, "TrailRider")
        field.tap(); field.typeText("_x")
        app.buttons["Cancel"].tap()

        openEditor()
        XCTAssertEqual(field.value as? String, "TrailRider")
        field.doubleTap(); field.typeText("Bad Name")
        XCTAssertEqual(field.value as? String, "Bad Name")
        app.buttons["Save"].tap()
        XCTAssertTrue(app.staticTexts["offline-player-name-error"].waitForExistence(timeout: 5))
        XCTAssertEqual(field.value as? String, "Bad Name")
        app.buttons["Cancel"].tap()
        openEditor()
        XCTAssertEqual(field.value as? String, "TrailRider")
        app.buttons["Cancel"].tap()
    }
    func testLauncherSettingsKeepOnlyGameFilesAndAbout() {
        let app = launch(signedIn: false, optionsResponse: "error")
        app.tabBars.buttons["Settings"].tap()
        XCTAssertTrue(app.staticTexts["Game files"].waitForExistence(timeout: 8))
        XCTAssertTrue(app.staticTexts["About"].exists)
        for title in ["Video", "Sound", "Controls", "Input", "iPad keyboard + pointer lock"] {
            XCTAssertFalse(app.staticTexts[title].exists)
        }
        XCTAssertFalse(app.buttons["settings-retry"].exists)
    }

    func testNativeTabBarHasOnlyThreeDestinationsAndNavigates() {
        let app = launch(signedIn: false)
        let bar = app.tabBars.firstMatch
        XCTAssertTrue(bar.waitForExistence(timeout: 5))
        XCTAssertEqual(bar.buttons.count, 3)
        XCTAssertFalse(bar.buttons["Worlds"].exists)
        XCTAssertFalse(bar.buttons["Servers"].exists)
        XCTAssertFalse(app.otherElements["launcher-floating-dock"].exists)
        XCTAssertFalse(app.staticTexts["All Worlds"].exists)
        XCTAssertFalse(app.staticTexts["Jump back in"].exists)
        for page in ["Packs", "Settings", "Play"] {
            let button = bar.buttons[page]
            XCTAssertTrue(button.isHittable); button.tap()
            XCTAssertTrue(bar.buttons[page].isSelected)
        }
        let image = XCTAttachment(screenshot: app.screenshot())
        image.name = "Native three-tab navigation"; image.lifetime = .keepAlways; add(image)
    }

    func testAccountControlIsCircularAndLibraryRoutesAreAbsent() {
        let app = launch(signedIn: true)
        let avatar = app.buttons["launcher-account-button"]
        XCTAssertTrue(avatar.isHittable)
        XCTAssertEqual(avatar.frame.width, avatar.frame.height, accuracy: 0.5)
        XCTAssertGreaterThanOrEqual(avatar.frame.width, 44)
        XCTAssertLessThanOrEqual(avatar.frame.width, 54)
        XCTAssertFalse(app.buttons["launcher-nav-worlds"].exists)
        XCTAssertFalse(app.buttons["launcher-nav-servers"].exists)
        XCTAssertFalse(app.staticTexts["Jump back in"].exists)
        XCTAssertFalse(app.staticTexts["All Worlds"].exists)
        XCTAssertFalse(app.buttons["new-world-button"].exists)
        avatar.tap()
        XCTAssertTrue(app.staticTexts["Microsoft account"].firstMatch.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["Change Offline Name"].exists)
    }

}
