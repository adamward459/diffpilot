import AppKit
import Foundation
import SwiftUI
import Testing

@testable import diffpilot

@MainActor
struct SidebarPerformanceBaselineTests {
    private static let baselineURL = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent(
        "PerfBaseline.json")
    private static let tolerance = 0.05

    private static func scenarios() -> [String: MetricCounts] {
        let sample = SampleChanges.sample(count: 10_000)
        let changes = sample.changes
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
        result["toggle-uncommitted"] = SidebarMetrics.measure {
            _ = FileTree.filter(root, FileFilter(onlyUncommitted: true))
        }
        result["toggle-hide-viewed"] = SidebarMetrics.measure {
            _ = FileTree.list(files, FileFilter(hideViewed: true), viewed: sample.viewed)
        }
        return result
    }

    private static func outlineScenarios() -> (counts: [String: MetricCounts], visibleRows: Int) {
        let sample = SampleChanges.sample(count: 5_000)
        let marks = FileMarks(viewed: sample.viewed, comments: sample.comments)
        let root = FileTree.build(sample.changes)
        let files = FileTree.files(in: root)
        let controller = OutlineController()
        let scroll = controller.makeScrollView()
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 272, height: 700), styleMask: [.titled], backing: .buffered,
            defer: false)
        window.contentView = scroll
        let outline = scroll.documentView as! NSOutlineView
        func input(_ filter: FileFilter, _ mode: SidebarMode = .tree) -> FileOutline {
            FileOutline(root: root, files: files, filter: filter, mode: mode, marks: marks, selection: .constant(nil))
        }
        var result: [String: MetricCounts] = [:]

        result["outline-expand-all"] = SidebarMetrics.measure {
            controller.update(input(FileFilter(query: "/")))
            window.layoutIfNeeded()
            scroll.layoutSubtreeIfNeeded()
        }
        let rows = outline.numberOfRows
        let visible = outline.rows(in: scroll.contentView.bounds).length
        result["outline-scroll"] = SidebarMetrics.measure {
            var row = 0
            while row < rows {
                row = min(row + visible, rows - 1)
                outline.scrollRowToVisible(row)
                scroll.layoutSubtreeIfNeeded()
                if row == rows - 1 { break }
            }
        }
        result["outline-type-char"] = SidebarMetrics.measure {
            controller.update(input(FileFilter(query: "/s")))
            scroll.layoutSubtreeIfNeeded()
        }
        result["outline-toggle-hide-viewed"] = SidebarMetrics.measure {
            controller.update(input(FileFilter(query: "/s", hideViewed: true)))
            scroll.layoutSubtreeIfNeeded()
        }
        result["outline-list-mode"] = SidebarMetrics.measure {
            controller.update(input(FileFilter(), .list))
            scroll.layoutSubtreeIfNeeded()
        }
        return (result, visible)
    }

    @Test func outlineReusesCellsAndReloadsOncePerChange() {
        let (counts, visible) = Self.outlineScenarios()
        for name in [
            "outline-expand-all", "outline-scroll", "outline-type-char", "outline-toggle-hide-viewed",
            "outline-list-mode",
        ] {
            #expect(
                counts[name]?.cellsCreated ?? .max <= visible + 40,
                "\(name) created \(counts[name]?.cellsCreated ?? -1) cells for \(visible) visible rows")
        }
        for name in ["outline-expand-all", "outline-type-char", "outline-toggle-hide-viewed", "outline-list-mode"] {
            #expect(counts[name]?.reloads == 1, "\(name) reloaded \(counts[name]?.reloads ?? -1) times")
        }
    }

    @Test func workCountsMatchCommittedBaseline() throws {
        var actual = Self.scenarios()
        actual.merge(Self.outlineScenarios().counts) { $1 }
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

        let expected = try #require(
            try Self.readBaseline()["unit"],
            "No unit baseline in PerfBaseline.json. Re-record with DIFFPILOT_RECORD_BASELINE=1.")
        var failures: [String] = []
        for (name, counts) in actual.sorted(by: { $0.key < $1.key }) {
            guard let old = expected[name] else {
                failures.append("\(name): missing from baseline")
                continue
            }
            if counts.reloads != old.reloads {
                failures.append("\(name).reloads: baseline \(old.reloads), now \(counts.reloads)")
            }
            for (field, oldValue, newValue) in [
                ("nodesVisited", old.nodesVisited, counts.nodesVisited),
                ("cellsCreated", old.cellsCreated + 5, counts.cellsCreated),
                ("cellsConfigured", old.cellsConfigured, counts.cellsConfigured),
                ("matcherByteCompares", old.matcherByteCompares, counts.matcherByteCompares),
            ] where Double(newValue) > Double(oldValue) * (1 + Self.tolerance) {
                failures.append("\(name).\(field): baseline \(oldValue), now \(newValue)")
            }
        }
        #expect(
            failures.isEmpty,
            "Work counts grew more than 5%: \(failures.joined(separator: "; ")). If intended, re-record with DIFFPILOT_RECORD_BASELINE=1."
        )
    }

    static func readBaseline() throws -> [String: [String: MetricCounts]] {
        try JSONDecoder().decode([String: [String: MetricCounts]].self, from: Data(contentsOf: baselineURL))
    }
}
