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
        var themes: [PaneTheme] {
            commands.compactMap { if case .setTheme(let theme) = $0 { theme } else { nil } }
        }
        var fileStates: [String] {
            commands.compactMap {
                if case .fileState(let path, let viewed, let collapsed) = $0 {
                    "\(path) \(viewed) \(collapsed)"
                } else {
                    nil
                }
            }
        }
        var resets: Int {
            commands.filter { if case .reset = $0 { true } else { false } }.count
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
        #expect(model.accessibilitySummary == "top=a.ts; threads=1; viewed=0; scroll=; reply=one two three ")
    }

    @Test func summaryIncludesTheLatestScrollStats() {
        let model = makeModel(Recorder(), repo: nil)
        model.handle(.scrollStats(ScrollStats(frames: 120, medianMs: 8.3, p95Ms: 9.1, maxMs: 16.7)))
        #expect(model.accessibilitySummary.contains("scroll=120/8.30/9.10/16.70"))
    }

    @Test func benchScrollRunsAfterTheFirstPaintOnlyWhenAsked() async {
        let plain = Recorder()
        let quiet = DiffPaneModel(repo: nil, benchScrollDelay: nil, send: { plain.commands.append($0) })
        quiet.handle(.firstPaint)

        let bench = Recorder()
        let model = DiffPaneModel(repo: nil, benchScrollDelay: .zero, send: { bench.commands.append($0) })
        model.handle(.firstPaint)
        await settle { bench.commands.contains { if case .benchScroll = $0 { true } else { false } } }
        try? await Task.sleep(for: .milliseconds(30))

        #expect(!plain.commands.contains { if case .benchScroll = $0 { true } else { false } })
        #expect(bench.commands.contains { if case .benchScroll = $0 { true } else { false } })
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

    @Test func aReloadedPageGetsTheDiffAndThreadsAgain() async throws {
        let repo = try makeRepo(files: 2)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        await model.load()
        model.handle(.ready)
        model.handle(.createThread(.file(path: "f1.txt")))
        recorder.commands.removeAll()

        model.pageDidReload()
        model.handle(.needMore)
        #expect(recorder.loads.isEmpty)
        model.handle(.ready)
        #expect(recorder.loads == [["f0.txt", "f1.txt"]])
        #expect(recorder.threads.map(\.anchor) == [.file(path: "f1.txt")])
    }

    private let sampleTheme = PaneTheme(colors: [:], isDark: false, increaseContrast: false, reduceTransparency: false)

    @Test func sendsTheThemeWhenThePageIsReadyAndAgainOnlyWhenItChanges() {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.setTheme(sampleTheme)
        #expect(recorder.themes.isEmpty)
        model.handle(.ready)
        #expect(recorder.themes == [sampleTheme])
        model.setTheme(sampleTheme)
        #expect(recorder.themes.count == 1)
        var dark = sampleTheme
        dark.isDark = true
        model.setTheme(dark)
        #expect(recorder.themes == [sampleTheme, dark])
    }

    @Test func markingAFileViewedCollapsesItAndUnmarkingExpandsIt() {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.toggleViewed("a.ts")
        #expect(model.viewed.paths == ["a.ts"])
        model.toggleViewed("a.ts")
        #expect(model.viewed.paths.isEmpty)
        #expect(recorder.fileStates == ["a.ts true true", "a.ts false false"])
    }

    @Test func theViewedToggleFromThePageWritesThroughToTheSharedStore() {
        let recorder = Recorder()
        let store = ViewedStore()
        let key = ViewedKey(repo: "/r", branch: "b")
        let model = DiffPaneModel(
            repo: nil, viewed: ViewedFiles(key: key, store: store), send: { recorder.commands.append($0) })
        model.handle(.toggleViewed("a.ts"))
        #expect(store.load(key) == ["a.ts"])
        #expect(recorder.fileStates == ["a.ts true true"])
    }

    @Test func startsWithTheStoredViewedFiles() {
        let recorder = Recorder()
        let store = ViewedStore()
        let key = ViewedKey(repo: "/r", branch: "b")
        store.save(key, ["a.ts"])
        let model = DiffPaneModel(
            repo: nil, viewed: ViewedFiles(key: key, store: store), send: { recorder.commands.append($0) })
        model.handle(.ready)
        #expect(recorder.fileStates == ["a.ts true false"])
    }

    @Test func countsThreadsPerFile() {
        let model = makeModel(Recorder(), repo: nil)
        model.handle(.createThread(.file(path: "a.ts")))
        model.handle(.createThread(.line(path: "a.ts", side: .new, line: 3)))
        model.handle(.createThread(.file(path: "b.ts")))
        #expect(model.commentCounts == ["a.ts": 2, "b.ts": 1])
    }

    @Test func pageCollapseTogglesKeepTheViewedFlag() {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.handle(.toggleCollapsed("a.ts"))
        model.handle(.toggleCollapsed("a.ts"))
        #expect(recorder.fileStates == ["a.ts false true", "a.ts false false"])
    }

    @Test func aReloadedPageGetsTheFileStatesAgain() {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.toggleViewed("a.ts")
        model.handle(.ready)
        #expect(recorder.fileStates == ["a.ts true true", "a.ts true true"])
    }

    @Test func commentOnVisibleFileOpensAFileThread() {
        let recorder = Recorder()
        let model = makeModel(recorder, repo: nil)
        model.commentOnVisibleFile()
        #expect(recorder.threads.isEmpty)
        model.handle(.visibleFile("a.ts"))
        model.commentOnVisibleFile()
        #expect(recorder.threads.first?.anchor == .file(path: "a.ts"))
    }

    @Test func reloadingAnUnchangedDiffSendsNothing() async throws {
        let repo = try makeRepo(files: 2)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        await model.load()
        model.handle(.ready)
        let sent = recorder.commands.count
        await model.reload()
        #expect(recorder.commands.count == sent)
    }

    @Test func reloadingAChangedDiffResetsThePageAndSendsTheFilesAgain() async throws {
        let repo = try makeRepo(files: 2)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        await model.load()
        model.handle(.ready)
        model.handle(.createThread(.file(path: "f0.txt")))
        try "base\nagain\n".write(to: repo.appendingPathComponent("f1.txt"), atomically: true, encoding: .utf8)
        await model.reload()
        #expect(recorder.resets == 1)
        #expect(recorder.loads == [["f0.txt", "f1.txt"], ["f0.txt", "f1.txt"]])
        #expect(recorder.threads.count == 2)
        let resetIndex = recorder.commands.firstIndex { if case .reset = $0 { true } else { false } }
        let lastLoadIndex = recorder.commands.lastIndex { if case .load = $0 { true } else { false } }
        #expect((resetIndex ?? .max) < (lastLoadIndex ?? .min))
    }

    @Test func reloadSendsTheFilesThroughTheVisibleOneSoThePageCanKeepItsPlace() async throws {
        let repo = try makeRepo(files: 3)
        defer { try? FileManager.default.removeItem(at: repo) }
        for i in 0..<3 {
            let text = (0..<DiffPaneModel.lineBudget).map { "line \($0) \(i)" }.joined(separator: "\n") + "\n"
            try text.write(to: repo.appendingPathComponent("f\(i).txt"), atomically: true, encoding: .utf8)
        }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        await model.load()
        model.handle(.ready)
        model.handle(.visibleFile("f2.txt"))
        try "other\n".write(to: repo.appendingPathComponent("f0.txt"), atomically: true, encoding: .utf8)
        await model.reload()
        #expect(recorder.resets == 1)
        #expect(recorder.loads.last?.last == "f2.txt")
    }

    @Test func reloadBeforeThePageIsReadyJustReplacesTheFiles() async throws {
        let repo = try makeRepo(files: 1)
        defer { try? FileManager.default.removeItem(at: repo) }
        let recorder = Recorder()
        let model = makeModel(recorder, repo: repo)
        await model.load()
        try "base\nnew\n".write(to: repo.appendingPathComponent("f0.txt"), atomically: true, encoding: .utf8)
        await model.reload()
        model.handle(.ready)
        #expect(recorder.resets == 0)
        #expect(recorder.loads == [["f0.txt"]])
    }
}
