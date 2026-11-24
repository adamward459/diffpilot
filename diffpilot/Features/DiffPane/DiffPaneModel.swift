import Foundation
import Observation
import os

@Observable
final class DiffPaneModel {
    static let lineBudget = 2500
    static let signposter = OSSignposter(subsystem: "com.adamward459.diffpilot", category: "DiffPane")
    static let firstPaintSignpost: StaticString = "firstPaint"

    var send: (HostCommand) -> Void
    private(set) var threads = ThreadStore()
    private(set) var firstPaintAt: Date?
    private(set) var visiblePath: String?
    private(set) var scrollStats: ScrollStats?

    private let repo: URL?
    private let base: String
    private let replier: any ClaudeReplier
    private let benchScrollDelay: Duration?
    private var batches: DiffBatches?
    private var isLoaded = false
    private var isPageReady = false
    private var hasSentFirstBatch = false
    private var inset = 0.0
    private var pendingReveal: String?
    private var firstPaintInterval: OSSignpostIntervalState?

    init(
        repo: URL?, base: String = "main", replier: any ClaudeReplier = ScriptedReplier(),
        benchScrollDelay: Duration? = ProcessInfo.processInfo.environment["DIFFPILOT_BENCH"].flatMap(Double.init).map {
            .seconds($0)
        },
        send: @escaping (HostCommand) -> Void = { _ in }
    ) {
        self.repo = repo
        self.base = base
        self.replier = replier
        self.benchScrollDelay = benchScrollDelay
        self.send = send
        firstPaintInterval = Self.signposter.beginInterval(Self.firstPaintSignpost)
    }

    var accessibilitySummary: String {
        let reply = threads.threads.values.flatMap(\.comments).last(where: { $0.author == ThreadStore.claudeName })
        let scroll =
            scrollStats.map { String(format: "%d/%.2f/%.2f/%.2f", $0.frames, $0.medianMs, $0.p95Ms, $0.maxMs) } ?? ""
        return
            "top=\(visiblePath ?? ""); threads=\(threads.threads.count); scroll=\(scroll); reply=\(reply?.body ?? "")"
    }

    func load() async {
        let repo = repo
        let base = base
        let files = await Task.detached(priority: .userInitiated) {
            guard let repo, let text = try? Git.run(repo, UnifiedDiff.args + [base]) else { return [FileDiff]() }
            return UnifiedDiff.parse(text).inTreeOrder()
        }.value
        batches = DiffBatches(files, lineBudget: Self.lineBudget)
        isLoaded = true
        sendFirstBatchIfReady()
    }

    func setInset(_ points: Double) {
        guard points != inset else { return }
        inset = points
        if isPageReady { send(.setInset(points)) }
    }

    func reveal(_ path: String) {
        guard hasSentFirstBatch else {
            pendingReveal = path
            return
        }
        if let batch = batches?.through(path: path) { send(.load(files: batch.files, done: batch.done)) }
        send(.reveal(path: path))
    }

    func handle(_ message: PageMessage) {
        switch message {
        case .ready:
            isPageReady = true
            send(.setInset(inset))
            sendFirstBatchIfReady()
        case .needMore:
            sendNextBatch()
        case .firstPaint:
            firstPaintAt = Date()
            if let interval = firstPaintInterval {
                Self.signposter.endInterval(Self.firstPaintSignpost, interval)
                firstPaintInterval = nil
            }
            if let delay = benchScrollDelay {
                Task {
                    try? await Task.sleep(for: delay)
                    send(.benchScroll)
                }
            }
        case .visibleFile(let path):
            visiblePath = path
        case .scrollStats(let stats):
            scrollStats = stats
        case .createThread(let anchor):
            send(.thread(threads.create(anchor)))
        case .post(let id, let text):
            guard let thread = threads.post(id, text: text) else { return }
            send(.thread(thread))
            if ThreadStore.mentionsClaude(text) { startReply(id) }
        case .askClaude(let id):
            startReply(id)
        case .remove(let id):
            threads.remove(id)
            send(.removeThread(id))
        }
    }

    private func sendFirstBatchIfReady() {
        guard isPageReady, isLoaded, !hasSentFirstBatch else { return }
        hasSentFirstBatch = true
        if let batch = batches?.next() {
            send(.load(files: batch.files, done: batch.done))
        } else {
            send(.load(files: [], done: true))
        }
        if let path = pendingReveal {
            pendingReveal = nil
            reveal(path)
        }
    }

    private func sendNextBatch() {
        guard hasSentFirstBatch, let batch = batches?.next() else { return }
        send(.load(files: batch.files, done: batch.done))
    }

    private func startReply(_ id: Int) {
        guard let thread = threads.beginClaudeReply(id) else { return }
        send(.thread(thread))
        let replier = replier
        Task {
            for await chunk in replier.reply(to: thread) {
                if let updated = threads.appendClaude(id, chunk, done: false) { send(.thread(updated)) }
            }
            if let updated = threads.appendClaude(id, "", done: true) { send(.thread(updated)) }
        }
    }
}
