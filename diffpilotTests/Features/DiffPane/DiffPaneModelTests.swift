import Foundation
import Testing

@testable import diffpilot

@MainActor
struct DiffPaneModelTests {
    final class Recorder {
        var commands: [HostCommand] = []

        var loads: [[String]] {
            commands.compactMap { if case .load(let files, _) = $0 { files.map(\.path) } else { nil } }
        }
        var reveals: [String] {
            commands.compactMap { if case .reveal(let path) = $0 { path } else { nil } }
        }
        var threads: [ReviewThread] {
            commands.compactMap { if case .thread(let thread) = $0 { thread } else { nil } }
        }
        var insets: [Double] {
            commands.compactMap { if case .setInset(let points) = $0 { points } else { nil } }
        }
    }

    private func makeRepo(files: Int) throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("pane-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        _ = try Git.run(url, ["init", "-q", "-b", "main"])
        for i in 0..<files {
            try "base\n".write(to: url.appendingPathComponent("f\(i).txt"), atomically: true, encoding: .utf8)
        }
        _ = try Git.run(url, ["add", "-A"])
        _ = try Git.run(url, ["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "base"])
        for i in 0..<files {
            try "base\nchanged \(i)\n".write(
                to: url.appendingPathComponent("f\(i).txt"), atomically: true, encoding: .utf8)
        }
        return url
    }

    private func makeModel(_ recorder: Recorder, repo: URL?, text: String = "one two three") -> DiffPaneModel {
        DiffPaneModel(
            repo: repo, replier: ScriptedReplier(text: text, interval: .zero),
            send: { recorder.commands.append($0) })
    }

    private func settle(_ condition: () -> Bool) async {
        for _ in 0..<500 where !condition() { try? await Task.sleep(for: .milliseconds(10)) }
    }

    @Test func sendsTheFirstBatchOnceTheDiffIsLoadedAndThePageIsReady() async throws {
        let repo = try makeRepo(files: 3)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        await model.load()
        #expect(recorder.loads.isEmpty)
        model.handle(.ready)
        #expect(recorder.loads == [["f0.txt", "f1.txt", "f2.txt"]])
    }

    @Test func waitsForTheDiffWhenThePageIsReadyFirst() async throws {
        let repo = try makeRepo(files: 1)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        model.handle(.ready)
        #expect(recorder.loads.isEmpty)
        await model.load()
        #expect(recorder.loads == [["f0.txt"]])
    }

    @Test func aRepoWithoutChangesSendsAnEmptyFinalBatch() async {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.handle(.ready)
        await model.load()
        #expect(recorder.loads == [[]])
    }

    @Test func needMoreSendsTheNextBatchUntilDone() async throws {
        let repo = try makeRepo(files: 3)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = DiffPaneModel(repo: repo, send: { recorder.commands.append($0) })
        await model.load()
        model.handle(.ready)
        model.handle(.needMore)
        model.handle(.needMore)
        #expect(recorder.loads == [["f0.txt", "f1.txt", "f2.txt"]])
    }

    @Test func revealSendsFilesThroughThePathAndThenScrolls() async throws {
        let repo = try makeRepo(files: 3)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        model.handle(.ready)
        await model.load()
        model.reveal("f1.txt")
        #expect(recorder.reveals == ["f1.txt"])
        #expect(recorder.loads.count == 1)
    }

    @Test func revealBeforeTheFirstBatchWaitsForIt() async throws {
        let repo = try makeRepo(files: 2)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        model.reveal("f1.txt")
        #expect(recorder.reveals.isEmpty)
        model.handle(.ready)
        await model.load()
        let order = recorder.commands.compactMap { command -> String? in
            switch command {
            case .load: "load"
            case .reveal: "reveal"
            default: nil
            }
        }
        #expect(order == ["load", "reveal"])
    }

    @Test func insetIsSentOnReadyAndWhenItChanges() {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.setInset(52)
        #expect(recorder.insets.isEmpty)
        model.handle(.ready)
        model.setInset(60)
        model.setInset(60)
        #expect(recorder.insets == [52, 60])
    }

    @Test func createThreadSendsADraftSnapshot() {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.handle(.createThread(.file(path: "a.ts")))
        #expect(recorder.threads.count == 1)
        #expect(recorder.threads[0].anchor == .file(path: "a.ts"))
        #expect(recorder.threads[0].comments.isEmpty)
    }

    @Test func aPostWithoutAMentionGetsNoReply() async {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.handle(.createThread(.file(path: "a.ts")))
        model.handle(.post(thread: 1, text: "plain"))
        try? await Task.sleep(for: .milliseconds(50))
        #expect(recorder.threads.last?.comments.map(\.author) == ["You"])
    }

    @Test func aMentionStreamsClaudesReplyWordByWord() async {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.handle(.createThread(.file(path: "a.ts")))
        model.handle(.post(thread: 1, text: "@claude look"))
        await settle { recorder.threads.last?.comments.last?.isStreaming == false }
        let last = recorder.threads.last
        #expect(last?.comments.map(\.author) == ["You", "Claude"])
        #expect(last?.comments.last?.body == "one two three ")
        let partial = recorder.threads.filter {
            $0.comments.last?.author == "Claude" && !$0.comments.last!.body.isEmpty
        }
        #expect(partial.count > 2)
    }

    @Test func askClaudeStartsAReplyWithoutAPost() async {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.handle(.createThread(.line(path: "a.ts", side: .new, line: 3)))
        model.handle(.askClaude(thread: 1))
        await settle { recorder.threads.last?.comments.last?.isStreaming == false }
        #expect(recorder.threads.last?.comments.map(\.author) == ["Claude"])
    }

    @Test func summaryReportsTheTopFileThreadsAndTheLatestReply() async {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.handle(.visibleFile("a.ts"))
        model.handle(.createThread(.file(path: "a.ts")))
        model.handle(.askClaude(thread: 1))
        await settle { recorder.threads.last?.comments.last?.isStreaming == false }
        #expect(model.accessibilitySummary == "top=a.ts; threads=1; reply=one two three ")
    }

    @Test func removeDropsTheThreadAndTellsThePage() {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.handle(.createThread(.file(path: "a.ts")))
        model.handle(.remove(thread: 1))
        #expect(model.threads.threads.isEmpty)
        guard case .removeThread(1) = recorder.commands.last else {
            Issue.record("expected removeThread")
            return
        }
    }
}
