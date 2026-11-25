import XCTest

@testable import diffpilot

final class DiffPanePerformanceTests: XCTestCase {
    private var diffText = ""

    override func setUpWithError() throws {
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment["DIFFPILOT_HOTPATH"] == "1",
            "Host-dependent; runs only under scripts/test.py hotpath")
        diffText = try Git.run(URL(fileURLWithPath: Fixture.large), UnifiedDiff.args + ["main"])
    }

    private var options: XCTMeasureOptions {
        let options = XCTMeasureOptions()
        options.iterationCount = 10
        return options
    }

    func testParseLargeDiff() {
        var files: [FileDiff] = []
        measure(metrics: [XCTClockMetric(), XCTMemoryMetric()], options: options) {
            files = UnifiedDiff.parse(diffText)
        }
        XCTAssertEqual(files.count, 2000)
        XCTAssertGreaterThan(files.reduce(0) { $0 + $1.additions + $1.deletions }, 40_000)
    }

    func testOrderAndBatchLargeDiff() {
        let parsed = UnifiedDiff.parse(diffText)
        var batches = 0
        measure(metrics: [XCTClockMetric()], options: options) {
            var queue = DiffBatches(parsed.inTreeOrder(), lineBudget: DiffPaneModel.lineBudget)
            batches = 0
            while queue.next() != nil { batches += 1 }
        }
        XCTAssertGreaterThan(batches, 10)
    }

    func testEncodeFirstBatch() throws {
        var queue = DiffBatches(UnifiedDiff.parse(diffText).inTreeOrder(), lineBudget: DiffPaneModel.lineBudget)
        let first = try XCTUnwrap(queue.next())
        var bytes = 0
        measure(metrics: [XCTClockMetric()], options: options) {
            bytes =
                HostCommand.load(files: first.files, done: false).call.arguments["json"].map { "\($0)".utf8.count } ?? 0
        }
        XCTAssertGreaterThan(bytes, 1000)
    }
}
