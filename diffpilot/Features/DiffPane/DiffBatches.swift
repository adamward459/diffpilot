nonisolated struct DiffBatches {
    private let files: [FileDiff]
    private let lineBudget: Int
    private var sent = 0

    init(_ files: [FileDiff], lineBudget: Int) {
        self.files = files
        self.lineBudget = lineBudget
    }

    mutating func next() -> (files: ArraySlice<FileDiff>, done: Bool)? {
        guard sent < files.count else { return nil }
        var end = sent
        var lines = 0
        while end < files.count, lines < lineBudget || end == sent {
            lines += files[end].additions + files[end].deletions
            end += 1
        }
        return take(upTo: end)
    }

    mutating func through(path: String) -> (files: ArraySlice<FileDiff>, done: Bool)? {
        guard let index = files[sent...].firstIndex(where: { $0.path == path }) else { return nil }
        return take(upTo: index + 1)
    }

    private mutating func take(upTo end: Int) -> (files: ArraySlice<FileDiff>, done: Bool) {
        defer { sent = end }
        return (files[sent..<end], end >= files.count)
    }
}
