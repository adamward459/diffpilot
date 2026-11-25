import XCTest

@testable import diffpilot

// A separate type so its parse runs after DiffPanePerformanceTests and does not raise their peak memory.
final class FingerprintPerformanceTests: XCTestCase {
    func testFingerprintLargeDiff() throws {
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment["DIFFPILOT_HOTPATH"] == "1",
            "Host-dependent; runs only under scripts/test.py hotpath")
        let parsed = UnifiedDiff.parse(
            try Git.run(URL(fileURLWithPath: Fixture.large), UnifiedDiff.args + ["main"]))
        let options = XCTMeasureOptions()
        options.iterationCount = 10
        var prints: [String] = []
        measure(metrics: [XCTClockMetric()], options: options) {
            prints = parsed.map(\.fingerprint)
        }
        XCTAssertEqual(Set(prints).count, 2000)
    }
}
