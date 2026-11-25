import Foundation
import Observation

nonisolated enum ThreadAnchor: Hashable, Sendable, Codable {
    enum Side: String, Codable, Sendable { case old, new }

    case file(path: String)
    case line(path: String, side: Side, line: Int)
    case change

    var path: String {
        switch self {
        case .file(let path), .line(let path, _, _): path
        case .change: ""
        }
    }

    private enum Key: String, CodingKey { case kind, path, side, line }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "file": self = .file(path: try c.decode(String.self, forKey: .path))
        case "line":
            self = .line(
                path: try c.decode(String.self, forKey: .path), side: try c.decode(Side.self, forKey: .side),
                line: try c.decode(Int.self, forKey: .line))
        case "change": self = .change
        case let other: throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: other)
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Key.self)
        switch self {
        case .file(let path):
            try c.encode("file", forKey: .kind)
            try c.encode(path, forKey: .path)
        case .line(let path, let side, let line):
            try c.encode("line", forKey: .kind)
            try c.encode(path, forKey: .path)
            try c.encode(side, forKey: .side)
            try c.encode(line, forKey: .line)
        case .change:
            try c.encode("change", forKey: .kind)
        }
    }
}

nonisolated struct FindingMeta: Equatable, Sendable, Codable {
    var severity: Severity
    var title: String
    var location: String?
}

nonisolated struct ThreadComment: Equatable, Sendable, Codable {
    var id: Int
    var author: String
    var body: String
    var isStreaming: Bool
}

nonisolated struct ReviewThread: Equatable, Sendable, Codable, Identifiable {
    var id: Int
    var anchor: ThreadAnchor
    var comments: [ThreadComment]
    var finding: FindingMeta?
}

@Observable
final class ThreadStore {
    static let userName = "You"
    static let claudeName = "Claude"

    private(set) var threads: [Int: ReviewThread] = [:]
    private var nextThreadID = 1

    func create(_ anchor: ThreadAnchor) -> ReviewThread {
        if let existing = threads.values.first(where: { $0.anchor == anchor }) { return existing }
        let thread = ReviewThread(id: nextThreadID, anchor: anchor, comments: [])
        nextThreadID += 1
        threads[thread.id] = thread
        return thread
    }

    func addFinding(_ anchor: ThreadAnchor, finding: FindingMeta?, body: String) -> ReviewThread {
        let thread = ReviewThread(
            id: nextThreadID, anchor: anchor,
            comments: [ThreadComment(id: 1, author: Self.claudeName, body: body, isStreaming: false)],
            finding: finding)
        nextThreadID += 1
        threads[thread.id] = thread
        return thread
    }

    func post(_ id: Int, text: String) -> ReviewThread? {
        append(id, author: Self.userName, body: text, isStreaming: false)
    }

    func beginClaudeReply(_ id: Int) -> ReviewThread? {
        guard let thread = threads[id], !thread.comments.contains(where: \.isStreaming) else { return nil }
        return append(id, author: Self.claudeName, body: "", isStreaming: true)
    }

    func appendClaude(_ id: Int, _ chunk: String, done: Bool) -> ReviewThread? {
        guard var thread = threads[id],
            let index = thread.comments.lastIndex(where: { $0.author == Self.claudeName && $0.isStreaming })
        else { return nil }
        thread.comments[index].body += chunk
        if done { thread.comments[index].isStreaming = false }
        threads[id] = thread
        return thread
    }

    func remove(_ id: Int) {
        threads[id] = nil
    }

    nonisolated static func mentionsClaude(_ text: String) -> Bool {
        text.range(of: #"(?<![\w@])@claude\b"#, options: [.regularExpression, .caseInsensitive]) != nil
    }

    private func append(_ id: Int, author: String, body: String, isStreaming: Bool) -> ReviewThread? {
        guard var thread = threads[id] else { return nil }
        let commentID = (thread.comments.map(\.id).max() ?? 0) + 1
        thread.comments.append(ThreadComment(id: commentID, author: author, body: body, isStreaming: isStreaming))
        threads[id] = thread
        return thread
    }
}
