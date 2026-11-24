import XCTest

final class SidebarPerformanceUITests: XCTestCase {
    override func setUpWithError() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["DIFFPILOT_PERF_UI"] == "1", "Set DIFFPILOT_PERF_UI=1 to run host-dependent measurements")
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
            app.launchEnvironment["DIFFPILOT_SAMPLE_COUNT"] = "5000"
            app.launch()
        }
    }

    func testScrollingHitches() {
        let app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_SAMPLE_COUNT"] = "5000"
        app.launch()
        app.typeText("/")

        let sidebar = app.scrollViews.firstMatch

        measure(metrics: [XCTOSSignpostMetric.scrollingAndDecelerationMetric, XCTCPUMetric(application: app)], options: options) {
            sidebar.swipeUp(velocity: .fast)
        }
    }
}
