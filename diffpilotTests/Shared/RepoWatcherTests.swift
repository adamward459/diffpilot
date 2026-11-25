import Foundation
import Testing

@testable import diffpilot

struct RepoWatcherFilterTests {
    @Test(arguments: [
        "src/a.ts", "a.ts", ".gitignore", ".github/workflows/ci.yml", ".git/HEAD", ".git/index",
        ".git/packed-refs", ".git/refs/heads/main", ".git/refs/remotes/origin/main",
    ])
    func keepsWorkingTreeAndGitStatePaths(path: String) {
        #expect(RepoWatcher.isRelevantPath(path))
    }

    @Test(arguments: [
        ".git", ".git/", ".git/objects/ab/cdef", ".git/index.lock", ".git/HEAD.lock", ".git/logs/HEAD",
        ".git/refs", ".git/FETCH_HEAD", ".git/COMMIT_EDITMSG", ".git/config", "vendor/lib/.git/HEAD",
        "vendor/lib/.git/index",
    ])
    func dropsGitInternals(path: String) {
        #expect(!RepoWatcher.isRelevantPath(path))
    }

    @Test func reportsHeadChangesAndIgnoresPathsOutsideTheRoot() {
        let root = "/r"
        #expect(RepoWatcher.summarize(["/r/.git/HEAD"], root: root) == (true, true))
        #expect(RepoWatcher.summarize(["/r/src/a.ts", "/r/.git/objects/x"], root: root) == (true, false))
        #expect(RepoWatcher.summarize(["/r/.git/objects/x", "/other/a.ts"], root: root) == (false, false))
    }
}

struct RepoWatcherTests {
    private func makeRepo() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("watch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        _ = try Git.run(url, ["init", "-q", "-b", "main"])
        return url
    }

    private func firstChange(in repo: URL, whileDoing touch: @escaping @Sendable () -> Void) async -> RepoChange? {
        await withTaskGroup(of: RepoChange?.self) { group in
            group.addTask {
                for await change in RepoWatcher.changes(in: repo, latency: 0.1) { return change }
                return nil
            }
            group.addTask {
                for _ in 0..<40 {
                    try? await Task.sleep(for: .milliseconds(250))
                    touch()
                }
                return nil
            }
            let first = await group.next() ?? nil
            group.cancelAll()
            return first
        }
    }

    @Test func reportsAWorkingTreeEditWithoutAHeadChange() async throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        let file = repo.appendingPathComponent("a.txt")
        let change = await firstChange(in: repo) {
            try? "x \(UUID())".write(to: file, atomically: true, encoding: .utf8)
        }
        #expect(change == RepoChange(headChanged: false))
    }

    @Test func reportsAHeadChange() async throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        let head = repo.appendingPathComponent(".git/HEAD")
        let change = await firstChange(in: repo) {
            try? "ref: refs/heads/other-\(UUID().uuidString.prefix(4))\n".write(
                to: head, atomically: true, encoding: .utf8)
        }
        #expect(change == RepoChange(headChanged: true))
    }
}
