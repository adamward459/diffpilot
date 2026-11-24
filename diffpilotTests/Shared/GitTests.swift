import Foundation
import Testing

@testable import diffpilot

struct GitTests {
    private func makeRepo() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("git-tests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        _ = try Git.run(url, ["init", "-q", "-b", "main"])
        return url
    }

    @Test func runsGitInTheRepo() throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        let branch = try Git.run(repo, ["symbolic-ref", "--short", "HEAD"])
        #expect(branch.trimmingCharacters(in: .whitespacesAndNewlines) == "main")
    }

    @Test func throwsGitsMessageOnFailure() throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        #expect {
            try Git.run(repo, ["rev-parse", "no-such-ref"])
        } throws: { error in
            if case Git.Failure.failed(let message) = error { return message.contains("no-such-ref") }
            return false
        }
    }
}
