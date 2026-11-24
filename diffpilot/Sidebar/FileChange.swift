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
    var isViewed: Bool
    var isUncommitted: Bool
    var commentCount: Int
}

nonisolated struct ChangeSummary: Equatable, Sendable {
    var fileCount = 0
    var additions = 0
    var deletions = 0
    var viewed = 0

    init() {}

    init(_ changes: [FileChange]) {
        fileCount = changes.count
        for change in changes {
            additions += change.additions
            deletions += change.deletions
            if change.isViewed { viewed += 1 }
        }
    }
}
