import XCTest

final class SidebarScreenshotUITests: XCTestCase {
    @MainActor
    func testSidebarLight() throws {
        try capture(appearance: "light")
    }

    @MainActor
    func testSidebarDark() throws {
        try capture(appearance: "dark")
    }

    @MainActor
    private func capture(appearance: String) throws {
        let app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_FIXTURE"] = "design"
        app.launchEnvironment["DIFFPILOT_APPEARANCE"] = appearance
        app.launch()
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 5))
        attach(window, named: "sidebar-\(appearance)")

        let filter = app.textFields.firstMatch
        XCTAssertTrue(filter.waitForExistence(timeout: 5))
        filter.click()
        filter.typeText("archive")
        attach(window, named: "sidebar-filter-\(appearance)")
    }

    private func attach(_ window: XCUIElement, named name: String) {
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
