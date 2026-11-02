import XCTest

final class OrientationPresentationTests: XCTestCase {
    func testLauncherPortraitAndGameLandscapePresentationHandoff() {
        XCUIDevice.shared.orientation = .portrait
        let app = XCUIApplication()
        let root = NSTemporaryDirectory() + "horizon-orientation-" + UUID().uuidString
        app.launchEnvironment = ["PREVIEW_GAME_PRESENTATION": "1", "HORIZON_NO_AUDIO": "1",
            "HORIZON_HOME": root + "/game", "HORIZON_LAUNCHER_HOME": root + "/launcher",
            "XDG_DATA_HOME": root + "/data", "XDG_CONFIG_HOME": root + "/config",
            "HORIZON_LAUNCHER_TRASH": root + "/trash", "HORIZON_LAUNCHER_CLIPBOARD": root + "/clipboard",
            "HORIZON_LAUNCHER_DESKTOP": root + "/desktop", "HORIZON_LAUNCHER_NO_SELF_UPDATE": "1"]
        app.launch()
        XCTAssertTrue(app.otherElements["launcher-presentation-permission-ready"].waitForExistence(timeout: 5))
        let play = app.buttons["launcher-play-button"]
        XCTAssertTrue(play.waitForExistence(timeout: 15))
        let portrait = NSPredicate { _, _ in app.frame.height > app.frame.width }
        expectation(for: portrait, evaluatedWith: app)
        waitForExpectations(timeout: 8)
        XCTAssertTrue(play.isHittable)
        XCTAssertGreaterThanOrEqual(play.frame.minX, app.frame.minX)
        XCTAssertLessThanOrEqual(play.frame.maxX, app.frame.maxX)
        XCTAssertLessThanOrEqual(play.frame.maxY, app.frame.maxY)
        for title in ["Play", "Packs", "Settings"] {
            let tab = app.tabBars.buttons[title]
            XCTAssertTrue(tab.isHittable)
            XCTAssertLessThanOrEqual(tab.frame.maxX, app.frame.maxX)
            XCTAssertLessThanOrEqual(tab.frame.maxY, app.frame.maxY)
        }
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = "Portrait-launcher-over-landscape-game"
        shot.lifetime = .keepAlways
        add(shot)
        play.tap()
        let game = app.staticTexts["orientation-game-root"]
        XCTAssertTrue(game.waitForExistence(timeout: 10))
        let landscape = NSPredicate { _, _ in app.frame.width > app.frame.height }
        expectation(for: landscape, evaluatedWith: app)
        waitForExpectations(timeout: 8)
        XCTAssertTrue(game.isHittable)
    }

}
