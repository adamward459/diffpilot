nonisolated extension FileStatus: Encodable {}

nonisolated struct FileDiff: Equatable, Sendable, Encodable {
    var path: String
    var oldPath: String?
    var status: FileStatus
    var isBinary = false
    var additions = 0
    var deletions = 0
    var hunks: [Hunk] = []
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
