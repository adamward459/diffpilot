import Foundation
import Testing

@testable import diffpilot

@MainActor
final class ComparisonTests {
    private let repo: URL
    private let suite = "ComparisonTests-\(UUID().uuidString)"
    private let defaults: UserDefaults

    init() throws {
        repo = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defaults = UserDefaults(suiteName: suite)!
        try FileManager.default.createDirectory(at: repo, withIntermediateDirectories: true)
        try git("init", "-q", "-b", "main")
        try write("base.txt", "base\n")
        try commit("base")
        try git("checkout", "-q", "-b", "other")
        try write("other.txt", "other\n")
        try commit("other")
        try git("checkout", "-q", "main")
        try git("checkout", "-q", "-b", "feature")
        try write("committed.txt", "committed\n")
        try commit("committed")
        try write("staged.txt", "staged\n")
        try git("add", "staged.txt")
        try write("base.txt", "base\nedited\n")
    }

    deinit {
        try? FileManager.default.removeItem(at: repo)
        UserDefaults().removePersistentDomain(forName: suite)
    }

    @Test(arguments: [
        (Comparison.checkedOut(base: "main", includesUncommitted: true), ["base.txt", "committed.txt", "staged.txt"]),
        (.checkedOut(base: "main", includesUncommitted: false), ["committed.txt"]),
        (.workingTree, ["base.txt", "staged.txt"]),
        (.staged, ["staged.txt"]),
        (.branch("other", base: "main"), ["other.txt"]),
        (.checkedOut(base: "other", includesUncommitted: false), ["committed.txt"]),
    ])
    func showsTheFilesOfEachComparison(comparison: Comparison, paths: [String]) throws {
        #expect(try RepoChanges.load(repo: repo, comparison: comparison).map(\.path) == paths)
    }

    @Test func marksUncommittedFilesOnlyWhenTheyAreIncluded() throws {
        let all = try RepoChanges.load(repo: repo, comparison: .checkedOut(base: "main", includesUncommitted: true))
        #expect(all.filter(\.isUncommitted).map(\.path) == ["base.txt", "staged.txt"])
        let branch = try RepoChanges.load(repo: repo, comparison: .branch("feature", base: "main"))
        #expect(branch.filter(\.isUncommitted).isEmpty)
    }

    @Test func defaultsToTheDetectedBaseWithUncommittedChanges() async {
        let model = ComparisonModel(repo: repo, defaults: defaults)
        await model.load()
        #expect(model.refs?.checkedOut == "feature")
        #expect(Set(model.refs?.local ?? []) == ["other", "main"])
        #expect(model.comparison == .checkedOut(base: "main", includesUncommitted: true))
    }

    @Test func restoresTheChoiceForTheSameCheckedOutBranch() async throws {
        let first = ComparisonModel(repo: repo, defaults: defaults)
        await first.load()
        first.select(.branch("other", base: "main"))

        let again = ComparisonModel(repo: repo, defaults: defaults)
        await again.load()
        #expect(again.comparison == .branch("other", base: "main"))

        try git("checkout", "-q", "main")
        let onMain = ComparisonModel(repo: repo, defaults: defaults)
        await onMain.load()
        #expect(onMain.comparison == .checkedOut(base: "main", includesUncommitted: true))
    }

    @Test func fallsBackToTheDefaultWhenTheSavedRefIsGone() async throws {
        let first = ComparisonModel(repo: repo, defaults: defaults)
        await first.load()
        first.select(.checkedOut(base: "other", includesUncommitted: false))
        try git("branch", "-q", "-D", "other")

        let again = ComparisonModel(repo: repo, defaults: defaults)
        await again.load()
        #expect(again.comparison == .checkedOut(base: "main", includesUncommitted: true))
    }

    @Test func restoresACommitBaseOutsideTheRecentList() async throws {
        let sha = try git("rev-parse", "--short", "main").trimmingCharacters(in: .whitespacesAndNewlines)
        ComparisonStore.save(
            .checkedOut(base: sha, includesUncommitted: true), repo: repo, checkedOut: "feature", defaults: defaults)
        let model = ComparisonModel(repo: repo, defaults: defaults)
        await model.load()
        #expect(model.comparison == .checkedOut(base: sha, includesUncommitted: true))
    }

    @discardableResult
    private func git(_ args: String...) throws -> String {
        try Git.run(repo, ["-c", "user.name=t", "-c", "user.email=t@t"] + args)
    }

    private func write(_ name: String, _ text: String) throws {
        try text.write(to: repo.appendingPathComponent(name), atomically: true, encoding: .utf8)
    }

    private func commit(_ message: String) throws {
        try git("add", "-A")
        try git("commit", "-q", "-m", message)
    }
}

struct RefsTests {
    @Test func prefersOriginHeadThenMainThenMaster() {
        let refs = "refs/heads/feature\nrefs/heads/master\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\n"
        #expect(
            Refs.parse(refs: refs, head: "feature\n", originHead: "origin/main\n", log: "").detectedBase
                == "origin/main")
        #expect(Refs.parse(refs: refs, head: "feature\n", originHead: "", log: "").detectedBase == "master")
    }

    @Test func groupsRefsAndHidesTheCheckedOutBranchAndOriginHead() {
        let refs = Refs.parse(
            refs:
                "refs/heads/feature\nrefs/heads/main\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\nrefs/tags/v1\n",
            head: "feature\n", originHead: "origin/main\n", log: "abc1234\tfeat: one\ndef5678\t\n")
        #expect(refs.checkedOut == "feature")
        #expect(refs.local == ["main"])
        #expect(refs.remote == ["origin/main"])
        #expect(refs.tags == ["v1"])
        #expect(refs.commits == [.init(sha: "abc1234", subject: "feat: one"), .init(sha: "def5678", subject: "")])
        #expect(refs.suggested == ["origin/main", "main"])
    }

    @Test func detachedHeadHasNoCheckedOutBranch() {
        let refs = Refs.parse(refs: "refs/heads/main\n", head: "", originHead: "", log: "")
        #expect(refs.checkedOut == nil)
        #expect(refs.detectedBase == "main")
    }
}
