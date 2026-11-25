import XCTest

final class AppUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testLaunchShowsMainWindow() throws {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 5))
    }

    @MainActor
    func testLaunchWithoutFolderOffersOpenFolder() throws {
        let app = XCUIApplication()
        app.launchArguments = ["-recentFolderBookmark", ""]
        app.launch()
        XCTAssertTrue(app.buttons["Open Folder…"].waitForExistence(timeout: 5))
    }
}
