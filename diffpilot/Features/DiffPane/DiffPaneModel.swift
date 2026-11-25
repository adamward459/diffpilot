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
    private(set) var collapsed: Set<String> = []
    private(set) var review = ReviewRun.idle
    var reviewFocuses: Set<String>
    var reviewInstructions = ""
    var reviewScope = ReviewScope.all
    var reviewModel = ReviewModel.standard
    var branchName: String?
    var mode = PaneMode.diff

    let viewed: ViewedFiles
    private let repo: URL?
    private let comparison: Comparison
    private let replier: any ClaudeReplier
    private let reviewer: any ClaudeReviewer
    private var reviewTask: Task<Void, Never>?
    private var reviewThreadIDs: [Int] = []
    private let focusDefaults: UserDefaults
    private let benchScrollDelay: Duration?
    private var files: [FileDiff]?
    private var batches: DiffBatches?
    private var isPageReady = false
    private var hasSentFirstBatch = false
    private var inset = 0.0
    private var theme: PaneTheme?
    private var pendingReveal: String?
    private var firstPaintInterval: OSSignpostIntervalState?

    init(
        repo: URL?, comparison: Comparison = .checkedOut(base: "main", includesUncommitted: true),
        viewed: ViewedFiles = ViewedFiles(key: ViewedKey(repo: "", branch: ""), store: ViewedStore()),
        replier: any ClaudeReplier = ScriptedReplier(),
        reviewer: any ClaudeReviewer = ScriptedReviewer(),
        focusDefaults: UserDefaults = .standard,
        benchScrollDelay: Duration? = ProcessInfo.processInfo.environment["DIFFPILOT_BENCH"].flatMap(Double.init).map {
            .seconds($0)
        },
        send: @escaping (HostCommand) -> Void = { _ in }
    ) {
        self.repo = repo
        self.comparison = comparison
        self.viewed = viewed
        collapsed = viewed.paths
        self.replier = replier
        self.reviewer = reviewer
        self.focusDefaults = focusDefaults
        reviewFocuses = repo.map { ReviewFocus.saved(repo: $0, defaults: focusDefaults) } ?? []
        self.benchScrollDelay = benchScrollDelay
        self.send = send
        firstPaintInterval = Self.signposter.beginInterval(Self.firstPaintSignpost)
        SymbolImages.warmUp()
    }

    var commentCounts: [String: Int] {
        threads.threads.values.reduce(into: [:]) { counts, thread in counts[thread.anchor.path, default: 0] += 1 }
    }

    var accessibilitySummary: String {
        let reply = threads.threads.values.filter { $0.finding == nil }.flatMap(\.comments).last(where: {
            $0.author == ThreadStore.claudeName
        })
        let scroll =
            scrollStats.map { String(format: "%d/%.2f/%.2f/%.2f", $0.frames, $0.medianMs, $0.p95Ms, $0.maxMs) } ?? ""
        return
            "top=\(visiblePath ?? ""); threads=\(threads.threads.count); viewed=\(viewed.paths.count); scroll=\(scroll); reply=\(reply?.body ?? ""); review=\(review.label)"
    }

    func load() async {
        let next = await readFiles()
        files = next
        batches = DiffBatches(next, lineBudget: Self.lineBudget)
        sendFirstBatchIfReady()
        await clearChangedViewed(next)
    }

    func reload() async {
        let next = await readFiles()
        guard next != files else { return }
        files = next
        await clearChangedViewed(next)
        batches = DiffBatches(next, lineBudget: Self.lineBudget)
        guard hasSentFirstBatch else {
            sendFirstBatchIfReady()
            return
        }
        send(.reset)
        hasSentFirstBatch = false
        sendFirstBatchIfReady()
        if let path = visiblePath, let batch = batches?.through(path: path) {
            send(.load(files: batch.files, done: batch.done))
        }
    }

    private func readFiles() async -> [FileDiff] {
        let repo = repo
        let comparison = comparison
        return await Task.detached(priority: .userInitiated) {
            guard let repo, let text = try? Git.run(repo, UnifiedDiff.args + comparison.revisions) else {
                return []
            }
            return UnifiedDiff.parse(text).inTreeOrder()
        }.value
    }

    private func clearChangedViewed(_ files: [FileDiff]) async {
        let viewed = viewed.paths
        let fingerprints = await Task.detached(priority: .utility) {
            Dictionary(
                files.filter { viewed.contains($0.path) }.map { ($0.path, $0.fingerprint) },
                uniquingKeysWith: { _, last in last })
        }.value
        for path in self.viewed.clearChanged(fingerprints) {
            collapsed.remove(path)
            if isPageReady { sendFileState(path) }
        }
    }

    func pageDidReload() {
        isPageReady = false
        hasSentFirstBatch = false
        batches = files.map { DiffBatches($0, lineBudget: Self.lineBudget) }
    }

    func setTheme(_ next: PaneTheme) {
        guard next != theme else { return }
        theme = next
        if isPageReady { send(.setTheme(next)) }
    }

    func toggleViewed(_ path: String) {
        let fingerprint = files?.first { $0.path == path }?.fingerprint
        viewed.toggle(path, fingerprint: fingerprint ?? ViewedFiles.unknown)
        if viewed.contains(path) { collapsed.insert(path) } else { collapsed.remove(path) }
        sendFileState(path)
    }

    var fileCount: Int { files?.count ?? 0 }

    var reviewTitle: String {
        let count = "\(fileCount) \(fileCount == 1 ? "file" : "files")"
        switch comparison {
        case .branch(let branch, let base): return "\(branch) compared to \(base) · \(count)"
        case .checkedOut(let base, _): return "\(branchName ?? "Current branch") compared to \(base) · \(count)"
        case .workingTree: return "Working tree compared to HEAD · \(count)"
        case .staged: return "Staged changes · \(count)"
        }
    }

    func startReview() {
        if case .running = review { return }
        guard let repo else {
            review = .failed("Open a Git repository first.")
            return
        }
        ReviewFocus.save(reviewFocuses, repo: repo, defaults: focusDefaults)
        let (focus, instructions) = ReviewFocus.request(selected: reviewFocuses, text: reviewInstructions)
        let request = ReviewRequest(
            repo: repo, revisions: comparison.revisions, focus: focus,
            instructions: instructions,
            paths: reviewScope == .openFile ? visiblePath.map { [$0] } ?? [] : [], model: reviewModel)
        let since = Date()
        review = .running(since: since)
        let reviewer = reviewer
        reviewTask = Task {
            do {
                let result = try await reviewer.review(request)
                guard !Task.isCancelled else { return }
                showReview(result)
            } catch {
                guard !Task.isCancelled else { return }
                review = .failed((error as? LocalizedError)?.errorDescription ?? error.localizedDescription)
            }
        }
    }

    func stopReview() {
        guard case .running = review else { return }
        reviewTask?.cancel()
        reviewTask = nil
        review = .idle
    }

    private func showReview(_ result: ReviewResult) {
        for id in reviewThreadIDs {
            threads.remove(id)
            send(.removeThread(id))
        }
        reviewThreadIDs = []
        for planned in ReviewPlan.threads(for: result, files: files ?? []) {
            let thread = threads.addFinding(planned.anchor, finding: planned.finding, body: planned.body)
            reviewThreadIDs.append(thread.id)
            send(.thread(thread))
        }
        review = .done(counts: result.counts)
    }

    func commentOnVisibleFile() {
        if let path = visiblePath { handle(.createThread(.file(path: path))) }
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
            if let theme { send(.setTheme(theme)) }
            send(.setInset(inset))
            for path in viewed.paths.union(collapsed).sorted() { sendFileState(path) }
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
        case .toggleViewed(let path):
            toggleViewed(path)
        case .toggleCollapsed(let path):
            if collapsed.remove(path) == nil { collapsed.insert(path) }
            sendFileState(path)
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

    private func sendFileState(_ path: String) {
        send(.fileState(path: path, viewed: viewed.contains(path), collapsed: collapsed.contains(path)))
    }

    private func sendFirstBatchIfReady() {
        guard isPageReady, files != nil, !hasSentFirstBatch else { return }
        hasSentFirstBatch = true
        if let batch = batches?.next() {
            send(.load(files: batch.files, done: batch.done))
        } else {
            send(.load(files: [], done: true))
        }
        for thread in threads.threads.values.sorted(by: { $0.id < $1.id }) { send(.thread(thread)) }
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

enum PaneMode: String, CaseIterable {
    case diff = "Diff"
    case edit = "Edit"
}
