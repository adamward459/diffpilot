import Foundation

nonisolated final class FileNode: @unchecked Sendable {
    nonisolated enum Kind {
        case folder
        case file(FileChange)
    }

    let name: String
    let path: String
    let parentPath: String
    let lowerPath: String
    let nameStart: Int
    let kind: Kind
    let children: [FileNode]

    static let empty = FileNode(folder: "", path: "", children: [])

    init(folder name: String, path: String, children: [FileNode]) {
        self.name = name
        self.path = path
        self.parentPath = ""
        self.lowerPath = ""
        self.nameStart = 0
        self.kind = .folder
        self.children = children
    }

    init(file change: FileChange) {
        let path = change.path
        let slash = path.lastIndex(of: "/")
        let lower = path.lowercased()
        let lowerSlash = lower.utf8.lastIndex(of: UInt8(ascii: "/"))
        self.name = slash.map { String(path[path.index(after: $0)...]) } ?? path
        self.path = path
        self.parentPath = slash.map { String(path[..<$0]) } ?? ""
        self.lowerPath = lower
        self.nameStart = lowerSlash.map { lower.utf8.distance(from: lower.utf8.startIndex, to: $0) + 1 } ?? 0
        self.kind = .file(change)
        self.children = []
    }

    var isFolder: Bool {
        if case .folder = kind { return true }
        return false
    }

    var change: FileChange? {
        if case .file(let change) = kind { return change }
        return nil
    }
}

nonisolated struct FileFilter: Equatable, Sendable {
    var query = ""
    var onlyUncommitted = false
    var hideViewed = false

    var trimmedQuery: String { query.trimmingCharacters(in: .whitespaces) }
    var hasQuery: Bool { !trimmedQuery.isEmpty }
    var isActive: Bool { hasQuery || onlyUncommitted || hideViewed }
}

nonisolated struct FileMatcher {
    private let needle: [UInt8]

    init?(_ query: String) {
        let text = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !text.isEmpty else { return nil }
        needle = Array(text.utf8)
    }

    func score(_ node: FileNode) -> Int? {
        var path = node.lowerPath
        var compares = 0
        let result = path.withUTF8 { score($0, nameStart: node.nameStart, compares: &compares) }
        let total = compares
        SidebarMetrics.record { $0.matcherByteCompares += total }
        return result
    }

    private func score(_ path: UnsafeBufferPointer<UInt8>, nameStart: Int, compares: inout Int) -> Int? {
        let count = path.count
        guard needle.count <= count else { return nil }
        if let index = find(in: path, from: nameStart, compares: &compares) {
            return 3000 - (index - nameStart) + (index == nameStart ? 500 : 0) - count
        }
        if let index = find(in: path, from: 0, compares: &compares) {
            return 2000 - index - count
        }
        return fuzzy(in: path, nameStart: nameStart, compares: &compares)
    }

    private func find(in path: UnsafeBufferPointer<UInt8>, from start: Int, compares: inout Int) -> Int? {
        let last = path.count - needle.count
        guard start <= last else { return nil }
        for index in start...last {
            var offset = 0
            while offset < needle.count {
                compares += 1
                if path[index + offset] != needle[offset] { break }
                offset += 1
            }
            if offset == needle.count { return index }
        }
        return nil
    }

    private func fuzzy(in path: UnsafeBufferPointer<UInt8>, nameStart: Int, compares: inout Int) -> Int? {
        var matched = 0
        var previous = -2
        var score = 1000
        for index in 0..<path.count {
            compares += 1
            guard path[index] == needle[matched] else { continue }
            if index == previous + 1 { score += 15 }
            if index >= nameStart { score += 5 }
            if index == 0 || Self.isSeparator(path[index - 1]) { score += 10 }
            previous = index
            matched += 1
            if matched == needle.count { return min(score, 1900) - path.count }
        }
        return nil
    }

    private static func isSeparator(_ byte: UInt8) -> Bool {
        byte == UInt8(ascii: "/") || byte == UInt8(ascii: ".") || byte == UInt8(ascii: "-") || byte == UInt8(ascii: "_")
    }
}

nonisolated enum FileTree {
    private final class Builder {
        var folders: [String: Builder] = [:]
        var files: [FileNode] = []
    }

    static func build(_ changes: [FileChange]) -> FileNode {
        let root = Builder()
        SidebarMetrics.record { $0.nodesVisited += changes.count }
        for change in changes {
            var builder = root
            let parts = change.path.split(separator: "/")
            for part in parts.dropLast() {
                let key = String(part)
                if let next = builder.folders[key] {
                    builder = next
                } else {
                    let next = Builder()
                    builder.folders[key] = next
                    builder = next
                }
            }
            builder.files.append(FileNode(file: change))
        }
        return FileNode(folder: "", path: "", children: makeChildren(root, parentPath: ""))
    }

    static func files(in root: FileNode) -> [FileNode] {
        var result: [FileNode] = []
        func visit(_ node: FileNode) {
            if node.isFolder { node.children.forEach(visit) } else { result.append(node) }
        }
        visit(root)
        return result
    }

    static func filter(_ root: FileNode, _ filter: FileFilter) -> FileNode {
        guard filter.isActive else { return root }
        let matcher = FileMatcher(filter.query)
        var visited = 0
        defer { let total = visited; SidebarMetrics.record { $0.nodesVisited += total } }
        func prune(_ node: FileNode) -> FileNode? {
            visited += 1
            guard let change = node.change else {
                let kept = node.children.compactMap(prune)
                return kept.isEmpty ? nil : FileNode(folder: node.name, path: node.path, children: kept)
            }
            return accepts(node, change, filter, matcher) != nil ? node : nil
        }
        return FileNode(folder: "", path: "", children: root.children.compactMap(prune))
    }

    static func list(_ files: [FileNode], _ filter: FileFilter) -> [FileNode] {
        guard filter.isActive else { return files }
        let matcher = FileMatcher(filter.query)
        SidebarMetrics.record { $0.nodesVisited += files.count }
        var scored: [(score: Int, node: FileNode)] = []
        for node in files {
            if let change = node.change, let score = accepts(node, change, filter, matcher) {
                scored.append((score, node))
            }
        }
        if matcher != nil {
            scored.sort { $0.score != $1.score ? $0.score > $1.score : $0.node.path < $1.node.path }
        }
        return scored.map(\.node)
    }

    private static func accepts(_ node: FileNode, _ change: FileChange, _ filter: FileFilter, _ matcher: FileMatcher?) -> Int? {
        if filter.onlyUncommitted && !change.isUncommitted { return nil }
        if filter.hideViewed && change.isViewed { return nil }
        guard let matcher else { return 0 }
        return matcher.score(node)
    }

    private static func makeChildren(_ builder: Builder, parentPath: String) -> [FileNode] {
        var folders: [(key: String, node: FileNode)] = []
        for (name, child) in builder.folders {
            var title = name
            var path = parentPath.isEmpty ? name : parentPath + "/" + name
            var current = child
            while current.files.isEmpty, current.folders.count == 1, let (next, nested) = current.folders.first {
                title += "/" + next
                path += "/" + next
                current = nested
            }
            folders.append((title.lowercased(), FileNode(folder: title, path: path, children: makeChildren(current, parentPath: path))))
        }
        folders.sort { $0.key < $1.key }
        let files = builder.files
            .map { (key: $0.name.lowercased(), node: $0) }
            .sorted { $0.key != $1.key ? $0.key < $1.key : $0.node.path < $1.node.path }
        return folders.map(\.node) + files.map(\.node)
    }
}
