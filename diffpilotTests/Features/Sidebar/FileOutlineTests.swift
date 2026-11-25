import AppKit
import SwiftUI
import Testing

@testable import diffpilot

@MainActor
struct FileOutlineExpansionTests {
    private func change(_ path: String) -> FileChange {
        FileChange(path: path, status: .modified, additions: 1, deletions: 0, isUncommitted: false)
    }

    private func setUp() -> (OutlineController, NSOutlineView) {
        let controller = OutlineController()
        let scroll = controller.makeScrollView()
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 272, height: 700), styleMask: [.titled], backing: .buffered,
            defer: false)
        window.contentView = scroll
        return (controller, scroll.documentView as! NSOutlineView)
    }

    private func input(_ changes: [FileChange], _ filter: FileFilter = FileFilter()) -> FileOutline {
        let root = FileTree.build(changes)
        return FileOutline(
            root: root, files: FileTree.files(in: root), filter: filter, mode: .tree, selection: .constant(nil))
    }

    private func titles(_ outline: NSOutlineView) -> [String] {
        (0..<outline.numberOfRows).compactMap { (outline.item(atRow: $0) as? FileNode)?.name }
    }

    @Test func expandsNestedFoldersByDefault() {
        let (controller, outline) = setUp()
        controller.update(input([change("a/b/c/one.ts"), change("a/b/two.ts"), change("z/three.ts")]))
        #expect(titles(outline) == ["a/b", "c", "one.ts", "two.ts", "z", "three.ts"])
    }

    @Test func collapsedFolderStaysCollapsedAndNewFoldersExpand() {
        let (controller, outline) = setUp()
        let first = input([change("a/one.ts"), change("a/sub/x.ts"), change("b/two.ts")])
        controller.update(first)
        let a = first.root.children.first { $0.name == "a" }!
        outline.collapseItem(a)

        controller.update(input([change("a/one.ts"), change("a/sub/x.ts"), change("b/two.ts"), change("c/d/new.ts")]))
        #expect(titles(outline) == ["a", "b", "two.ts", "c/d", "new.ts"])
    }

    @Test func queryExpandsEverythingWithoutRecordingCollapse() {
        let (controller, outline) = setUp()
        let changes = [change("a/one.ts"), change("a/sub/one.ts"), change("b/two.ts")]
        let plain = input(changes)
        controller.update(plain)
        outline.collapseItem(plain.root.children.first { $0.name == "a" }!)

        controller.update(input(changes, FileFilter(query: "one")))
        #expect(titles(outline).filter { $0 == "one.ts" }.count == 2)

        controller.update(input(changes))
        #expect(titles(outline) == ["a", "b", "two.ts"])
    }

    private func marked(_ changes: [FileChange], _ marks: FileMarks, _ filter: FileFilter = FileFilter())
        -> FileOutline
    {
        let root = FileTree.build(changes)
        return FileOutline(
            root: root, files: FileTree.files(in: root), filter: filter, mode: .tree, marks: marks,
            selection: .constant(nil))
    }

    @Test func markChangesReloadOnlyTheChangedRows() {
        let (controller, outline) = setUp()
        let changes = (0..<30).map { change("src/f\($0).ts") }
        let root = FileTree.build(changes)
        func input(_ marks: FileMarks) -> FileOutline {
            FileOutline(
                root: root, files: FileTree.files(in: root), filter: FileFilter(), mode: .tree, marks: marks,
                selection: .constant(nil))
        }
        controller.update(input(FileMarks()))
        let rowsBefore = outline.numberOfRows

        let counts = SidebarMetrics.measure {
            controller.update(input(FileMarks(viewed: ["src/f3.ts"], comments: ["src/f7.ts": 2])))
        }
        #expect(counts.reloads == 0)
        #expect(counts.cellsConfigured == 2)
        #expect(outline.numberOfRows == rowsBefore)
    }

    @Test func hideViewedRefiltersWhenTheViewedSetChanges() {
        let (controller, outline) = setUp()
        let changes = [change("a/one.ts"), change("a/two.ts")]
        let filter = FileFilter(hideViewed: true)
        controller.update(marked(changes, FileMarks(), filter))
        #expect(titles(outline) == ["a", "one.ts", "two.ts"])

        let counts = SidebarMetrics.measure {
            controller.update(marked(changes, FileMarks(viewed: ["a/one.ts"]), filter))
        }
        #expect(counts.reloads == 1)
        #expect(titles(outline) == ["a", "two.ts"])
    }
}
