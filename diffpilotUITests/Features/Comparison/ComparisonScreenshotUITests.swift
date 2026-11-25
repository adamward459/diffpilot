import XCTest

final class ComparisonScreenshotUITests: XCTestCase {
    @MainActor
    func testComparisonLight() throws {
        try capture(appearance: "light")
    }

    @MainActor
    func testComparisonDark() throws {
        try capture(appearance: "dark")
    }

    @MainActor
    private func capture(appearance: String) throws {
        let app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_REPO"] = Fixture.large
        app.launchEnvironment["DIFFPILOT_VIEWED_SEED"] = Fixture.viewedSeed
        app.launchEnvironment["DIFFPILOT_APPEARANCE"] = appearance
        app.launch()
        let window = app.windows.firstMatch
        XCTAssertTrue(app.menuButtons["comparison-base"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts["summary"].waitForExistence(timeout: 10))
        let attachment = XCTAttachment(screenshot: try window.uncoveredScreenshot())
        attachment.name = "comparison-\(appearance)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
