import XCTest

final class DiffPaneScreenshotUITests: XCTestCase {
    @MainActor
    func testDiffPaneLight() throws {
        try capture(appearance: "light")
    }

    @MainActor
    func testDiffPaneDark() throws {
        try capture(appearance: "dark")
    }

    @MainActor
    private func capture(appearance: String) throws {
        let pane = DiffPaneDriver(appearance: appearance)
        pane.app.launchEnvironment["DIFFPILOT_REVIEW_SECONDS"] = "6"
        pane.app.launch()
        XCTAssertTrue(pane.state.waitForExistence(timeout: 10))
        pane.moveToPrimaryScreen()
        XCTAssertTrue(pane.waitUntil { pane.recognizedText().contains("archive.md") })
        try attach(pane.window, named: "diffpane-\(appearance)")

        pane.paneCoordinate(x: pane.paneFrame.midX, y: pane.paneFrame.midY).click()
        for _ in 0..<4 { pane.app.typeKey(.downArrow, modifierFlags: []) }
        Thread.sleep(forTimeInterval: 1)
        Thread.sleep(forTimeInterval: 1.5)
        try attach(pane.window, named: "diffpane-pinned-\(appearance)")
        pane.app.typeKey(.home, modifierFlags: [])
        Thread.sleep(forTimeInterval: 1.5)

        pane.hoverAndClickLineButton(line: 2)
        XCTAssertTrue(pane.waitUntil { pane.threadCount == 1 })
        pane.app.typeText("@claude is this line needed?")
        pane.app.typeKey(.return, modifierFlags: .command)
        XCTAssertTrue(
            pane.waitUntil(timeout: 20, pollEvery: 0.5) { pane.reply.contains("did not change any code yet.") })
        XCTAssertTrue(pane.waitUntil { pane.recognizedText().contains("change any code") })
        try attach(pane.window, named: "diffpane-thread-\(appearance)")

        pane.paneCoordinate(x: pane.paneFrame.minX + 15, y: pane.paneFrame.midY).click()
        pane.app.typeKey(.home, modifierFlags: [])
        Thread.sleep(forTimeInterval: 1.5)
        pane.startReview()
        XCTAssertTrue(pane.waitUntil(pollEvery: 0.5) { pane.review == "running" })
        Thread.sleep(forTimeInterval: 1.5)
        try attach(pane.window, named: "diffpane-reviewing-\(appearance)")
        XCTAssertTrue(pane.waitUntil(timeout: 20, pollEvery: 0.5) { pane.review.hasPrefix("done") })
        // The pill shows the result for 5 s only, so capture it first.
        try attach(pane.window, named: "diffpane-review-done-\(appearance)")
        XCTAssertTrue(pane.waitUntil { pane.recognizedText().contains("Review summary") })
        try attach(pane.window, named: "diffpane-review-\(appearance)")
    }

    private func attach(_ window: XCUIElement, named name: String) throws {
        let attachment = XCTAttachment(screenshot: try window.uncoveredScreenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
