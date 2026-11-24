import Foundation

nonisolated enum PageMessage: Equatable, Sendable {
    case ready
    case needMore
    case firstPaint
    case createThread(ThreadAnchor)
    case post(thread: Int, text: String)
    case askClaude(thread: Int)
    case remove(thread: Int)

    init?(body: Any) {
        guard let fields = body as? [String: Any], let type = fields["type"] as? String else { return nil }
        switch type {
        case "ready": self = .ready
        case "needMore": self = .needMore
        case "firstPaint": self = .firstPaint
        case "createThread":
            guard let path = fields["path"] as? String else { return nil }
            if let side = (fields["side"] as? String).flatMap(ThreadAnchor.Side.init(rawValue:)),
                let line = fields["line"] as? Int
            {
                self = .createThread(.line(path: path, side: side, line: line))
            } else {
                self = .createThread(.file(path: path))
            }
        case "post":
            guard let thread = fields["thread"] as? Int, let text = fields["text"] as? String else { return nil }
            self = .post(thread: thread, text: text)
        case "askClaude":
            guard let thread = fields["thread"] as? Int else { return nil }
            self = .askClaude(thread: thread)
        case "remove":
            guard let thread = fields["thread"] as? Int else { return nil }
            self = .remove(thread: thread)
        default: return nil
        }
    }
}

nonisolated enum HostCommand: Sendable {
    case load(files: ArraySlice<FileDiff>, done: Bool)
    case thread(ReviewThread)
    case removeThread(Int)
    case reveal(path: String)
    case setInset(Double)

    var call: (script: String, arguments: [String: Any]) {
        switch self {
        case .load(let files, let done):
            ("dp.load(JSON.parse(json), done)", ["json": Self.json(Array(files)), "done": done])
        case .thread(let thread):
            ("dp.thread(JSON.parse(json))", ["json": Self.json(thread)])
        case .removeThread(let id):
            ("dp.removeThread(id)", ["id": id])
        case .reveal(let path):
            ("dp.reveal(path)", ["path": path])
        case .setInset(let points):
            ("dp.setInset(points)", ["points": points])
        }
    }

    private static func json(_ value: some Encodable) -> String {
        (try? JSONEncoder().encode(value)).map { String(decoding: $0, as: UTF8.self) } ?? "null"
    }
}
