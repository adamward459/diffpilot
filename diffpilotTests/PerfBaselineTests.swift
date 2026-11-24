import Foundation
import Testing
@testable import diffpilot

struct PerfBaselineTests {
    private static let baselineURL = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("PerfBaseline.json")
    private static let tolerance = 0.05

    private static func scenarios() -> [String: MetricCounts] {
        let changes = SampleChanges.generate(count: 10_000)
        var root = FileNode.empty
        var files: [FileNode] = []
        var result: [String: MetricCounts] = [:]

        result["build"] = SidebarMetrics.measure {
            root = FileTree.build(changes)
            files = FileTree.files(in: root)
        }
        result["filter-uau"] = SidebarMetrics.measure { _ = FileTree.filter(root, FileFilter(query: "uau")) }
        result["filter-src-api"] = SidebarMetrics.measure { _ = FileTree.filter(root, FileFilter(query: "src/api")) }
        result["list-uau"] = SidebarMetrics.measure { _ = FileTree.list(files, FileFilter(query: "uau")) }
        result["toggle-uncommitted"] = SidebarMetrics.measure { _ = FileTree.filter(root, FileFilter(onlyUncommitted: true)) }
        result["toggle-hide-viewed"] = SidebarMetrics.measure { _ = FileTree.list(files, FileFilter(hideViewed: true)) }
        return result
    }

    @Test func workCountsMatchCommittedBaseline() throws {
        let actual = Self.scenarios()
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]

        if ProcessInfo.processInfo.environment["DIFFPILOT_RECORD_BASELINE"] == "1" {
            var file = (try? Self.readBaseline()) ?? [:]
            file["unit"] = actual
            let out = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("PerfBaseline.json")
            try encoder.encode(file).write(to: out)
            Issue.record("Recorded to \(out.path). Copy it over \(Self.baselineURL.path).")
            return
        }

        let expected = try #require(try Self.readBaseline()["unit"], "No unit baseline in PerfBaseline.json. Re-record with DIFFPILOT_RECORD_BASELINE=1.")
        var failures: [String] = []
        for (name, counts) in actual.sorted(by: { $0.key < $1.key }) {
            guard let old = expected[name] else {
                failures.append("\(name): missing from baseline")
                continue
            }
            for (field, oldValue, newValue) in [
                ("nodesVisited", old.nodesVisited, counts.nodesVisited),
                ("matcherByteCompares", old.matcherByteCompares, counts.matcherByteCompares),
            ] where Double(newValue) > Double(oldValue) * (1 + Self.tolerance) {
                failures.append("\(name).\(field): baseline \(oldValue), now \(newValue)")
            }
        }
        #expect(failures.isEmpty, "Work counts grew more than 5%: \(failures.joined(separator: "; ")). If intended, re-record with DIFFPILOT_RECORD_BASELINE=1.")
    }

    static func readBaseline() throws -> [String: [String: MetricCounts]] {
        try JSONDecoder().decode([String: [String: MetricCounts]].self, from: Data(contentsOf: baselineURL))
    }
}
