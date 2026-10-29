import XCTest

final class LauncherLayoutTests: XCTestCase {
    private func launch(signedIn: Bool, optionsResponse: String? = nil) -> XCUIApplication {
        let app = XCUIApplication()
        let root = NSTemporaryDirectory() + "horizon-native-" + UUID().uuidString
        app.launchEnvironment = ["PREVIEW_SIGNED_IN": signedIn ? "1" : "0",
            "HORIZON_HOME": root + "/game", "HORIZON_LAUNCHER_HOME": root + "/launcher",
            "XDG_DATA_HOME": root + "/data", "XDG_CONFIG_HOME": root + "/config",
            "HORIZON_LAUNCHER_TRASH": root + "/trash", "HORIZON_LAUNCHER_CLIPBOARD": root + "/clipboard",
            "HORIZON_LAUNCHER_DESKTOP": root + "/desktop", "HORIZON_LAUNCHER_NO_SELF_UPDATE": "1", "HORIZON_NO_AUDIO": "1"]
        if let optionsResponse { app.launchEnvironment["PREVIEW_OPTIONS_RESPONSE"] = optionsResponse }
        app.launch()
        XCTAssertTrue(app.buttons["launcher-play-button"].waitForExistence(timeout: 15))
        return app
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
    func testOfflineNameEditing() {
        let app = launch(signedIn: false)
        app.buttons["launcher-account-button"].tap()
        let edit = app.buttons["Change Offline Name"]
        XCTAssertTrue(edit.waitForExistence(timeout: 5)); edit.tap()
        let alert = app.alerts["Offline player name"]
        XCTAssertTrue(alert.waitForExistence(timeout: 5))
        alert.textFields.firstMatch.tap()
        alert.buttons["Cancel"].tap()
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
