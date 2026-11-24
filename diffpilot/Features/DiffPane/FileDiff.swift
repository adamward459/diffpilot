import Foundation

nonisolated extension FileStatus: Encodable {}

nonisolated struct FileDiff: Equatable, Sendable, Encodable {
    var path: String
    var oldPath: String?
    var status: FileStatus
    var isBinary = false
    var additions = 0
    var deletions = 0
    var hunks: [Hunk] = []

    private enum CodingKeys: String, CodingKey {
        case path, oldPath, status, isBinary, additions, deletions, hunks, symbol, tint
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(path, forKey: .path)
        try container.encodeIfPresent(oldPath, forKey: .oldPath)
        try container.encode(status, forKey: .status)
        try container.encode(isBinary, forKey: .isBinary)
        try container.encode(additions, forKey: .additions)
        try container.encode(deletions, forKey: .deletions)
        try container.encode(hunks, forKey: .hunks)
        let icon = FileIcon.forFile(named: (path as NSString).lastPathComponent)
        try container.encode(icon.symbol, forKey: .symbol)
        try container.encode(icon.tint.rawValue, forKey: .tint)
    }
}

nonisolated struct Hunk: Equatable, Sendable, Encodable {
    var header: String
    var lines: [DiffLine] = []
}

nonisolated struct DiffLine: Equatable, Sendable, Encodable {
    enum Kind: String, Sendable {
        case context = "c"
        case added = "a"
        case deleted = "d"
    }

    var kind: Kind
    var old: Int?
    var new: Int?
    var text: String

    func encode(to encoder: Encoder) throws {
        var container = encoder.unkeyedContainer()
        try container.encode(kind.rawValue)
        try container.encode(old)
        try container.encode(new)
        try container.encode(text)
    }
}

extension Array where Element == FileDiff {
    nonisolated func inTreeOrder() -> [FileDiff] {
        let keys = map { file in
            (
                parts: file.path.split(separator: "/", omittingEmptySubsequences: false).map { $0.lowercased() },
                path: file.path
            )
        }
        let order = indices.sorted { a, b in
            let (left, right) = (keys[a], keys[b])
            for i in 0..<Swift.min(left.parts.count, right.parts.count) where left.parts[i] != right.parts[i] {
                let leftIsFolder = i < left.parts.count - 1
                let rightIsFolder = i < right.parts.count - 1
                if leftIsFolder != rightIsFolder { return leftIsFolder }
                return left.parts[i] < right.parts[i]
            }
            return left.path < right.path
        }
        return order.map { self[$0] }
    }
}
