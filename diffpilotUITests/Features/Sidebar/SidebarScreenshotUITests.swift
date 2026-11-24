import XCTest

final class SidebarScreenshotUITests: XCTestCase {
    @MainActor
    func testSidebar() throws {
        let app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_REPO"] = "large"
        app.launch()
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 5))
        attach(window, named: "sidebar")

        let filter = app.textFields.firstMatch
        XCTAssertTrue(filter.waitForExistence(timeout: 5))
        filter.click()
        filter.typeText("archive")
        attach(window, named: "sidebar-filter")
    }

    private func attach(_ window: XCUIElement, named name: String) {
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
