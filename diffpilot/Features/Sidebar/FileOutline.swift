import AppKit
import SwiftUI

enum SidebarMode {
    case tree
    case list
}

nonisolated struct FileMarks: Equatable, Sendable {
    var viewed: Set<String> = []
    var comments: [String: Int] = [:]
}

struct FileOutline: NSViewRepresentable {
    let root: FileNode
    let files: [FileNode]
    let filter: FileFilter
    let mode: SidebarMode
    let marks: FileMarks
    let toggleViewed: (String) -> Void
    @Binding var selection: String?

    init(
        root: FileNode, files: [FileNode], filter: FileFilter, mode: SidebarMode, marks: FileMarks = FileMarks(),
        toggleViewed: @escaping (String) -> Void = { _ in }, selection: Binding<String?>
    ) {
        self.root = root
        self.files = files
        self.filter = filter
        self.mode = mode
        self.marks = marks
        self.toggleViewed = toggleViewed
        _selection = selection
    }

    func makeCoordinator() -> OutlineController { OutlineController() }

    func makeNSView(context: Context) -> NSScrollView { context.coordinator.makeScrollView() }

    func updateNSView(_ view: NSScrollView, context: Context) { context.coordinator.update(self) }
}

final class OutlineController: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate, NSMenuDelegate {
    private let outline = NSOutlineView()
    private var top: [FileNode] = []
    private var collapsed: Set<String> = []
    private var isReloading = false
    private var isSelecting = false
    private var selection: Binding<String?> = .constant(nil)
    private var toggleViewed: (String) -> Void = { _ in }
    private var marks = FileMarks()

    private var shownRoot: FileNode?
    private var shownFilter: FileFilter?
    private var shownMode: SidebarMode?
    private var listMode = false

    func makeScrollView() -> NSScrollView {
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("name"))
        column.resizingMask = .autoresizingMask
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.headerView = nil
        outline.style = .sourceList
        outline.rowSizeStyle = .custom
        outline.rowHeight = 24
        outline.usesAutomaticRowHeights = false
        outline.intercellSpacing = .zero
        outline.indentationPerLevel = 14
        outline.backgroundColor = .clear
        outline.allowsMultipleSelection = false
        outline.setAccessibilityIdentifier("file-outline")
        let menu = NSMenu()
        menu.delegate = self
        outline.menu = menu
        outline.dataSource = self
        outline.delegate = self

        let scroll = NSScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        return scroll
    }

    func update(_ input: FileOutline) {
        selection = input.$selection
        toggleViewed = input.toggleViewed
        let structureChanged = shownRoot !== input.root || shownFilter != input.filter || shownMode != input.mode
        let marksChanged = marks != input.marks
        let previous = marks
        marks = input.marks
        if structureChanged {
            shownRoot = input.root
            shownFilter = input.filter
            shownMode = input.mode
            listMode = input.mode == .list
            rebuild(input)
        } else if marksChanged {
            if input.filter.hideViewed && previous.viewed != input.marks.viewed {
                rebuild(input)
            } else {
                reloadRows(for: changedPaths(previous, input.marks))
            }
        }
        select(path: input.selection)
    }

    private func rebuild(_ input: FileOutline) {
        top =
            listMode
            ? FileTree.list(input.files, input.filter, viewed: input.marks.viewed)
            : FileTree.filter(input.root, input.filter, viewed: input.marks.viewed).children
        reload(expandAll: !listMode && input.filter.hasQuery)
    }

    private func changedPaths(_ old: FileMarks, _ new: FileMarks) -> Set<String> {
        var paths = old.viewed.symmetricDifference(new.viewed)
        for path in Set(old.comments.keys).union(new.comments.keys) where old.comments[path] != new.comments[path] {
            paths.insert(path)
        }
        return paths
    }

    private func reloadRows(for paths: Set<String>) {
        var rows = IndexSet()
        for path in paths {
            guard let node = find(path, in: top) else { continue }
            let row = outline.row(forItem: node)
            if row >= 0 { rows.insert(row) }
        }
        guard !rows.isEmpty else { return }
        outline.reloadData(forRowIndexes: rows, columnIndexes: IndexSet(integer: 0))
    }

    private func reload(expandAll: Bool) {
        isReloading = true
        isSelecting = true
        SidebarMetrics.record { $0.reloads += 1 }
        outline.reloadData()
        if !listMode {
            // One bulk call is about 3x faster than expanding each folder on the 2,000-file fixture.
            outline.expandItem(nil, expandChildren: true)
            if !expandAll { collapseMarked(top) }
        }
        isSelecting = false
        isReloading = false
    }

    private func collapseMarked(_ nodes: [FileNode]) {
        for node in nodes where node.isFolder {
            collapseMarked(node.children)
            if collapsed.contains(node.path) { outline.collapseItem(node) }
        }
    }

    private func select(path: String?) {
        let current = outline.item(atRow: outline.selectedRow) as? FileNode
        guard current?.change?.path != path else { return }
        guard let path, let node = find(path, in: top) else {
            if current?.change != nil {
                isSelecting = true
                outline.deselectAll(nil)
                isSelecting = false
            }
            return
        }
        let row = outline.row(forItem: node)
        guard row >= 0 else { return }
        isSelecting = true
        outline.selectRowIndexes([row], byExtendingSelection: false)
        outline.scrollRowToVisible(row)
        isSelecting = false
    }

    private func find(_ path: String, in nodes: [FileNode]) -> FileNode? {
        for node in nodes {
            if node.change?.path == path { return node }
            if node.isFolder, path.hasPrefix(node.path + "/"), let found = find(path, in: node.children) {
                return found
            }
        }
        return nil
    }

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        guard let node = item as? FileNode else { return top.count }
        return node.children.count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        guard let node = item as? FileNode else { return top[index] }
        return node.children[index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        (item as? FileNode)?.isFolder ?? false
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? FileNode else { return nil }
        if let change = node.change {
            let cell = outlineView.makeView(withIdentifier: FileCell.id, owner: self) as? FileCell ?? FileCell()
            cell.configure(
                node: node, change: change, showsParent: listMode, isViewed: marks.viewed.contains(change.path),
                commentCount: marks.comments[change.path] ?? 0)
            return cell
        }
        let cell = outlineView.makeView(withIdentifier: FolderCell.id, owner: self) as? FolderCell ?? FolderCell()
        cell.configure(name: node.name, path: node.path)
        return cell
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard !isSelecting else { return }
        let node = outline.item(atRow: outline.selectedRow) as? FileNode
        if let path = node?.change?.path { selection.wrappedValue = path }
    }

    func outlineViewItemDidExpand(_ notification: Notification) {
        track(notification, expanded: true)
    }

    func outlineViewItemDidCollapse(_ notification: Notification) {
        track(notification, expanded: false)
    }

    private func track(_ notification: Notification, expanded isExpanded: Bool) {
        guard !isReloading, !listMode, shownFilter?.hasQuery != true,
            let node = notification.userInfo?["NSObject"] as? FileNode
        else { return }
        if isExpanded { collapsed.remove(node.path) } else { collapsed.insert(node.path) }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard let path = (outline.item(atRow: outline.clickedRow) as? FileNode)?.change?.path else { return }
        let title = marks.viewed.contains(path) ? "Mark as Not Viewed" : "Mark as Viewed"
        let item = NSMenuItem(title: title, action: #selector(toggleViewedFromMenu(_:)), keyEquivalent: "")
        item.target = self
        item.representedObject = path
        menu.addItem(item)
    }

    @objc private func toggleViewedFromMenu(_ item: NSMenuItem) {
        if let path = item.representedObject as? String { toggleViewed(path) }
    }
}
