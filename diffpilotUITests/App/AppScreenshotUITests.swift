import XCTest

final class AppScreenshotUITests: XCTestCase {
    @MainActor
    func testMainWindow() throws {
        let app = XCUIApplication()
        app.launch()
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 5))

        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = "main-window"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
