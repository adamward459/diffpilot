import Testing

@testable import diffpilot

struct DiffBatchesTests {
    private func file(_ path: String, lines: Int) -> FileDiff {
        FileDiff(path: path, status: .modified, additions: lines)
    }

    private func paths(_ batch: (files: ArraySlice<FileDiff>, done: Bool)?) -> [String]? {
        batch?.files.map(\.path)
    }

    @Test func fillsEachBatchToTheLineBudget() {
        var batches = DiffBatches(
            [file("a", lines: 6), file("b", lines: 6), file("c", lines: 6), file("d", lines: 1)], lineBudget: 10)
        let first = batches.next()
        let second = batches.next()
        let third = batches.next()
        #expect(paths(first) == ["a", "b"])
        #expect(first?.done == false)
        #expect(paths(second) == ["c", "d"])
        #expect(second?.done == true)
        #expect(third == nil)
    }

    @Test func aFileLargerThanTheBudgetGoesAlone() {
        var batches = DiffBatches([file("big", lines: 100), file("small", lines: 1)], lineBudget: 10)
        let first = batches.next()
        let second = batches.next()
        #expect(paths(first) == ["big"])
        #expect(paths(second) == ["small"])
    }

    @Test func throughSendsEverythingUpToThePath() {
        var batches = DiffBatches(
            [file("a", lines: 1), file("b", lines: 1), file("c", lines: 1), file("d", lines: 1)], lineBudget: 1)
        _ = batches.next()
        let revealed = batches.through(path: "c")
        let rest = batches.next()
        #expect(paths(revealed) == ["b", "c"])
        #expect(revealed?.done == false)
        #expect(paths(rest) == ["d"])
    }

    @Test func throughIgnoresFilesAlreadySentAndUnknownPaths() {
        var batches = DiffBatches([file("a", lines: 1), file("b", lines: 1)], lineBudget: 1)
        _ = batches.next()
        let sent = batches.through(path: "a")
        let unknown = batches.through(path: "nope")
        #expect(sent == nil)
        #expect(unknown == nil)
    }

    @Test func throughTheLastFileFinishes() {
        var batches = DiffBatches([file("a", lines: 1), file("b", lines: 1)], lineBudget: 1)
        let revealed = batches.through(path: "b")
        let rest = batches.next()
        #expect(revealed?.done == true)
        #expect(rest == nil)
    }

    @Test func noFilesMeansNoBatches() {
        var batches = DiffBatches([], lineBudget: 10)
        let batch = batches.next()
        #expect(batch == nil)
    }
}
