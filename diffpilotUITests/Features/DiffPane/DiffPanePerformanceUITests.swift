import XCTest

final class DiffPanePerformanceUITests: XCTestCase {
    override func setUpWithError() throws {
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment["DIFFPILOT_HOTPATH"] == "1",
            "Host-dependent; runs only under scripts/test.py hotpath")
        continueAfterFailure = false
    }

    private var options: XCTMeasureOptions {
        let options = XCTMeasureOptions()
        options.iterationCount = 3
        return options
    }

    func testLaunchToFirstPaint() {
        measure(metrics: [XCTApplicationLaunchMetric(waitUntilResponsive: true)], options: options) {
            let pane = DiffPaneDriver()
            pane.app.launch()
            XCTAssertTrue(pane.waitUntil { pane.topFile == "docs/archive.md" })
        }
    }

    func testScrollingHitches() {
        let pane = DiffPaneDriver()
        pane.app.launch()
        XCTAssertTrue(pane.waitUntil { pane.topFile == "docs/archive.md" })
        let target = pane.state

        measure(
            metrics: [XCTOSSignpostMetric.scrollingAndDecelerationMetric, XCTCPUMetric(application: pane.app)],
            options: options
        ) {
            target.swipeUp(velocity: .fast)
        }
        XCTAssertNotEqual(pane.topFile, "docs/archive.md")
    }
}
