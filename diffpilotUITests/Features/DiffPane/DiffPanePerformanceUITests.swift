import AppKit
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
        let firstPaint = XCTOSSignpostMetric(
            subsystem: "com.adamward459.diffpilot", category: "DiffPane", name: "firstPaint")
        measure(metrics: [XCTApplicationLaunchMetric(), firstPaint], options: options) {
            let pane = DiffPaneDriver()
            pane.app.launch()
            XCTAssertTrue(pane.state.waitForExistence(timeout: 10))
        }
    }

    private let benchDelay = 6

    func testScrollingKeepsTheDisplayRate() throws {
        var worst = 0.0
        var refresh = 60.0
        for _ in 0..<options.iterationCount {
            let pane = DiffPaneDriver()
            pane.app.launchEnvironment["DIFFPILOT_BENCH"] = "\(benchDelay)"
            pane.app.launch()
            XCTAssertTrue(pane.state.waitForExistence(timeout: 10))
            refresh = Double(pane.moveToFastestScreen().maximumFramesPerSecond)
            sleep(UInt32(benchDelay + 4))
            XCTAssertTrue(pane.waitUntil(timeout: 30, pollEvery: 1) { pane.scrollStats != nil }, pane.stateValue)
            let stats = try XCTUnwrap(pane.scrollStats)
            worst = max(worst, stats.p95)
            print(
                "scroll bench: frames=\(stats.frames) p50=\(stats.p50)ms p95=\(stats.p95)ms max=\(stats.max)ms display=\(refresh)Hz window=\(pane.window.frame)"
            )
            pane.app.terminate()
        }
        let budget = 1000 / refresh * 1.5
        XCTAssertLessThanOrEqual(worst, budget, "p95 frame time \(worst) ms; budget \(budget) ms at \(refresh) Hz")
    }
}
