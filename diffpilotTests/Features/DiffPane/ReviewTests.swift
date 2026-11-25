import Foundation
import Testing

@testable import diffpilot

@MainActor
struct ReviewTests {
    private let files = [
        FileDiff(
            path: "src/a.ts", status: .modified, additions: 2, deletions: 1,
            hunks: [
                Hunk(
                    header: "@@ -1,2 +1,3 @@",
                    lines: [
                        DiffLine(kind: .context, old: 1, new: 1, text: "keep"),
                        DiffLine(kind: .deleted, old: 2, new: nil, text: "gone"),
                        DiffLine(kind: .added, old: nil, new: 2, text: "new"),
                    ])
            ])
    ]

    private func finding(_ path: String, _ line: Int?, _ severity: Severity = .suggestion) -> Finding {
        Finding(path: path, line: line, severity: severity, title: "T", body: "B")
    }

    private func plan(_ findings: [Finding], summary: String = "") -> [PlannedThread] {
        ReviewPlan.threads(for: ReviewResult(summary: summary, findings: findings), files: files)
    }

    @Test func aFindingOnAShownLineAnchorsToThatLine() {
        #expect(plan([finding("src/a.ts", 2)]).map(\.anchor) == [.line(path: "src/a.ts", side: .new, line: 2)])
        #expect(plan([finding("./src/a.ts", 1)]).map(\.anchor) == [.line(path: "src/a.ts", side: .new, line: 1)])
    }

    @Test func aFindingOnALineOutsideTheDiffAnchorsToTheFileAndKeepsTheLineInItsLabel() {
        let planned = plan([finding("src/a.ts", 40)])
        #expect(planned.map(\.anchor) == [.file(path: "src/a.ts")])
        #expect(planned.first?.finding?.location == "Line 40")
    }

    @Test func aFindingWithoutALineAnchorsToTheFile() {
        let planned = plan([finding("src/a.ts", nil)])
        #expect(planned.map(\.anchor) == [.file(path: "src/a.ts")])
        #expect(planned.first?.finding?.location == nil)
    }

    @Test func aFindingOutsideTheDiffAnchorsToTheChangeAndNamesItsPlace() {
        let planned = plan([finding("other.py", 3, .question)])
        #expect(planned.map(\.anchor) == [.change])
        #expect(planned.first?.finding == FindingMeta(severity: .question, title: "T", location: "other.py:3"))
        #expect(plan([finding("other.py", nil)]).first?.finding?.location == "other.py")
    }

    @Test func theSummaryComesFirstAsAChangeThreadWithoutFindingMeta() {
        let planned = plan([finding("src/a.ts", 2)], summary: " All good. ")
        #expect(planned.first == PlannedThread(anchor: .change, finding: nil, body: "All good."))
        #expect(planned.count == 2)
        #expect(plan([], summary: "  ").isEmpty)
    }

    @Test func changeAnchorRoundTripsThroughCodableWithoutAPath() throws {
        let data = try JSONEncoder().encode(ThreadAnchor.change)
        let object = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["kind"] as? String == "change")
        #expect(object["path"] == nil)
        #expect(try JSONDecoder().decode(ThreadAnchor.self, from: data) == .change)
        let line = ThreadAnchor.line(path: "a", side: .old, line: 3)
        #expect(try JSONDecoder().decode(ThreadAnchor.self, from: JSONEncoder().encode(line)) == line)
    }

    @Test func aFindingThreadEncodesItsFindingForThePage() throws {
        let store = ThreadStore()
        let thread = store.addFinding(
            .change, finding: FindingMeta(severity: .mustFix, title: "Bad", location: nil), body: "Fix it")
        let json = try #require(HostCommand.thread(thread).call.arguments["json"] as? String)
        let object = try #require(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
        let finding = try #require(object["finding"] as? [String: Any])
        #expect(finding["severity"] as? String == "must-fix")
        #expect(finding["title"] as? String == "Bad")
        #expect((object["anchor"] as? [String: Any])?["kind"] as? String == "change")
    }

    @Test func findingsNeverShareAThreadEvenOnTheSameAnchor() {
        let store = ThreadStore()
        let a = store.addFinding(.change, finding: nil, body: "a")
        let b = store.addFinding(.change, finding: nil, body: "b")
        #expect(a.id != b.id)
        #expect(store.threads.count == 2)
    }

    @Test func parsesTheStructuredOutputOfTheCLI() throws {
        let json = """
            {"type":"result","subtype":"success","is_error":false,"result":"","session_id":"x",
             "structured_output":{"summary":"One bug.","findings":[
               {"path":"a.ts","line":4,"severity":"must-fix","title":"Null","body":"Check it."},
               {"path":"b.ts","line":null,"severity":"question","title":"Why","body":"Is it used?"},
               {"path":"c.ts","severity":"nit","title":"Name","body":"Rename."}]}}
            """
        let result = try ClaudeCLIReviewer.parse(Data(json.utf8))
        #expect(result.summary == "One bug.")
        #expect(result.findings.map(\.severity) == [.mustFix, .question, .nit])
        #expect(result.findings.map(\.line) == [4, nil, nil])
        #expect(result.counts == [.mustFix: 1, .question: 1, .nit: 1])
    }

    @Test func cliErrorsBecomeMessages() {
        #expect(throws: ClaudeCLIReviewer.Failure.failed("Not logged in")) {
            try ClaudeCLIReviewer.parse(Data(#"{"is_error":true,"result":"Not logged in"}"#.utf8))
        }
        #expect(throws: ClaudeCLIReviewer.Failure.failed("Claude returned no structured review.")) {
            try ClaudeCLIReviewer.parse(Data(#"{"is_error":false,"result":"hi"}"#.utf8))
        }
        #expect(throws: ClaudeCLIReviewer.Failure.failed("Claude returned output that is not valid JSON.")) {
            try ClaudeCLIReviewer.parse(Data("oops".utf8))
        }
    }

    @Test func cliArgumentsAllowOnlyReadingGit() {
        let args = ClaudeCLIReviewer.arguments(
            for: ReviewRequest(repo: URL(filePath: "/r"), revisions: ["--merge-base", "main"], focus: "Tests"))
        #expect(args.first == "-p")
        #expect(args[1].contains("git diff --merge-base main"))
        #expect(args.contains("--json-schema"))
        let allowed = args.drop { $0 != "--allowedTools" }.dropFirst()
        #expect(Array(allowed) == ["Bash(git diff:*)", "Bash(git log:*)", "Bash(git show:*)"])
    }

    @Test func cliArgumentsCarryTheChosenModelAndEffort() {
        var request = ReviewRequest(repo: URL(filePath: "/r"), revisions: ["--cached"], focus: "Tests")
        request.model = ReviewModel(name: .sonnet, effort: .medium)
        let args = ClaudeCLIReviewer.arguments(for: request)
        #expect(args.drop { $0 != "--model" }.dropFirst().first == "sonnet")
        #expect(args.drop { $0 != "--effort" }.dropFirst().first == "medium")
    }

    @Test func aScopedReviewDiffsOnlyThoseFilesAndSaysSo() {
        var request = ReviewRequest(repo: URL(filePath: "/r"), revisions: ["--merge-base", "main"], focus: "Tests")
        request.paths = ["src/a b.ts"]
        let prompt = ReviewPrompt.render(request)
        #expect(prompt.contains("git diff --merge-base main -- 'src/a b.ts'"))
        #expect(prompt.contains("## Scope"))
        #expect(!prompt.contains("{{"))
    }

    @Test func theRenderedPromptHasNoPlaceholdersLeft() {
        let plain = ReviewPrompt.render(
            ReviewRequest(repo: URL(filePath: "/r"), revisions: ["--cached"], focus: "everything"))
        #expect(!plain.contains("{{"))
        #expect(plain.contains("git diff --cached"))
        #expect(plain.contains("Focus: everything"))
        #expect(!plain.contains("The user's instructions"))
        let guided = ReviewPrompt.render(
            ReviewRequest(
                repo: URL(filePath: "/r"), revisions: ["HEAD"], focus: "Tests, Types",
                instructions: "Look at auth.\nAnd retries."))
        #expect(!guided.contains("{{"))
        #expect(guided.contains("> Look at auth.\n> And retries.\n"))
        #expect(guided.contains("takes priority over the generic passes in Step 2"))
    }

    @Test func focusRequestJoinsPickedChipsInPresetOrderAndTrimsTheText() {
        let picked = ReviewFocus.request(selected: ["Types", "Security", "Bogus"], text: "  hi \n")
        #expect(picked.focus == "Security, Types")
        #expect(picked.instructions == "hi")
        #expect(ReviewFocus.request(selected: [], text: "").focus == "everything")
    }

    @Test func pickedFocusesAreRememberedPerRepo() throws {
        let defaults = try #require(UserDefaults(suiteName: "ReviewTests-\(UUID().uuidString)"))
        let a = URL(filePath: "/repo/a")
        ReviewFocus.save(["Tests", "Security"], repo: a, defaults: defaults)
        #expect(ReviewFocus.saved(repo: a, defaults: defaults) == ["Tests", "Security"])
        #expect(ReviewFocus.saved(repo: URL(filePath: "/repo/b"), defaults: defaults).isEmpty)
    }

    // MARK: run state

    private func makeRepo() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("review-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        _ = try Git.run(url, ["init", "-q", "-b", "main"])
        try "base\n".write(to: url.appendingPathComponent("a.txt"), atomically: true, encoding: .utf8)
        _ = try Git.run(url, ["add", "-A"])
        _ = try Git.run(url, ["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "base"])
        try "base\nchanged\n".write(to: url.appendingPathComponent("a.txt"), atomically: true, encoding: .utf8)
        return url
    }

    private func makeModel(_ reviewer: ScriptedReviewer, repo: URL?, commands: ThreadLog = ThreadLog()) -> DiffPaneModel
    {
        DiffPaneModel(
            repo: repo, reviewer: reviewer,
            focusDefaults: UserDefaults(suiteName: "ReviewTests-\(UUID().uuidString)") ?? .standard,
            send: { commands.commands.append($0) })
    }

    final class ThreadLog {
        var commands: [HostCommand] = []
        var threadIDs: [Int] { commands.compactMap { if case .thread(let t) = $0 { t.id } else { nil } } }
        var removed: [Int] { commands.compactMap { if case .removeThread(let id) = $0 { id } else { nil } } }
    }

    private func settle(_ condition: () -> Bool) async {
        for _ in 0..<500 where !condition() { try? await Task.sleep(for: .milliseconds(10)) }
    }

    @Test func aReviewRunsThenShowsCountsAndThreads() async throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        let log = ThreadLog()
        let result = ReviewResult(
            summary: "Fine.", findings: [finding("a.txt", 2, .mustFix), finding("a.txt", nil, .nit)])
        let model = makeModel(ScriptedReviewer(result: result, delay: .zero), repo: repo, commands: log)
        await model.load()
        #expect(model.review == .idle)
        model.startReview()
        guard case .running = model.review else {
            Issue.record("not running: \(model.review)")
            return
        }
        await settle { if case .done = model.review { true } else { false } }
        #expect(model.review == .done(counts: [.mustFix: 1, .nit: 1]))
        #expect(model.threads.threads.count == 3)
        #expect(model.accessibilitySummary.hasSuffix("review=done:1/0/1/0"))
    }

    @Test func stopReturnsToIdleAndDropsALateResult() async throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        let model = makeModel(ScriptedReviewer(delay: .milliseconds(100)), repo: repo)
        model.startReview()
        model.stopReview()
        #expect(model.review == .idle)
        try? await Task.sleep(for: .milliseconds(300))
        #expect(model.review == .idle)
        #expect(model.threads.threads.isEmpty)
    }

    @Test func aFailureShowsItsMessageAndTryAgainWorks() async throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        let model = makeModel(ScriptedReviewer(delay: .zero, failure: "No network"), repo: repo)
        model.startReview()
        await settle { if case .failed = model.review { true } else { false } }
        #expect(model.review == .failed("No network"))
        model.startReview()
        guard case .running = model.review else {
            Issue.record("not running: \(model.review)")
            return
        }
    }

    @Test func aSecondRunReplacesTheFirstRunsThreadsButKeepsHumanOnes() async throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        let log = ThreadLog()
        let model = makeModel(
            ScriptedReviewer(result: ReviewResult(summary: "S", findings: [finding("a.txt", 2)]), delay: .zero),
            repo: repo, commands: log)
        await model.load()
        model.handle(.createThread(.file(path: "a.txt")))
        model.startReview()
        await settle { if case .done = model.review { true } else { false } }
        let firstRun = Set(model.threads.threads.keys)
        model.startReview()
        await settle { log.removed.count == 2 }
        await settle { if case .done = model.review { true } else { false } }
        #expect(model.threads.threads.count == 3)
        #expect(Set(log.removed).isSubset(of: firstRun))
        #expect(model.threads.threads.values.contains { $0.anchor == .file(path: "a.txt") && $0.finding == nil })
    }

    @Test func aRunWithoutARepoFails() {
        let model = makeModel(ScriptedReviewer(delay: .zero), repo: nil)
        model.startReview()
        #expect(model.review == .failed("Open a Git repository first."))
    }

    @Test func startingWhileRunningDoesNothing() async throws {
        let repo = try makeRepo()
        defer { try? FileManager.default.removeItem(at: repo) }
        let model = makeModel(ScriptedReviewer(delay: .milliseconds(200)), repo: repo)
        model.startReview()
        let first = model.review
        model.startReview()
        #expect(model.review == first)
        model.stopReview()
    }
}
