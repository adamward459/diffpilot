nonisolated enum FileStatus: String, Sendable {
    case added = "A"
    case modified = "M"
    case deleted = "D"
    case renamed = "R"
}

nonisolated struct FileChange: Hashable, Sendable {
    var path: String
    var status: FileStatus
    var additions: Int
    var deletions: Int
    var isUncommitted: Bool
}

nonisolated struct ChangeSummary: Equatable, Sendable {
    var fileCount = 0
    var additions = 0
    var deletions = 0
    var viewed = 0

    init() {}

    init(_ changes: [FileChange], viewed viewedPaths: Set<String> = []) {
        fileCount = changes.count
        for change in changes {
            additions += change.additions
            deletions += change.deletions
            if viewedPaths.contains(change.path) { viewed += 1 }
        }
    }
}
