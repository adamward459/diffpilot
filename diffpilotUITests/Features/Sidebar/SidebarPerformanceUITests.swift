import XCTest

final class SidebarPerformanceUITests: XCTestCase {
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

    func testLaunchTime() {
        measure(metrics: [XCTApplicationLaunchMetric()], options: options) {
            let app = XCUIApplication()
            app.launchEnvironment["DIFFPILOT_REPO"] = Fixture.large
            app.launchEnvironment["DIFFPILOT_VIEWED_SEED"] = Fixture.viewedSeed
            app.launch()
        }
    }

    func testScrollingHitches() {
        let app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_REPO"] = Fixture.large
        app.launchEnvironment["DIFFPILOT_VIEWED_SEED"] = Fixture.viewedSeed
        app.launch()
        let pane = app.otherElements["diff-pane"]
        XCTAssertTrue(pane.waitForExistence(timeout: 10))
        let loaded = NSPredicate(format: "label CONTAINS 'top=docs/'")
        expectation(for: loaded, evaluatedWith: pane)
        waitForExpectations(timeout: 15)
        app.typeText("/")

        let sidebar = app.scrollViews.firstMatch

        measure(
            metrics: [XCTOSSignpostMetric.scrollingAndDecelerationMetric, XCTCPUMetric(application: app)],
            options: options
        ) {
            sidebar.swipeUp(velocity: .fast)
        }
    }
}
