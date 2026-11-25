import CoreServices
import Foundation

nonisolated struct RepoChange: Equatable, Sendable {
    var headChanged: Bool
}

nonisolated enum RepoWatcher {
    static let latency: TimeInterval = 0.3

    static func changes(in repo: URL, latency: TimeInterval = latency) -> AsyncStream<RepoChange> {
        let root = repo.resolvingSymlinksInPath().path
        return AsyncStream(bufferingPolicy: .unbounded) { continuation in
            let box = Box { paths in
                let change = summarize(paths, root: root)
                if change.isRelevant { continuation.yield(RepoChange(headChanged: change.headChanged)) }
            }
            var context = FSEventStreamContext(
                version: 0, info: Unmanaged.passUnretained(box).toOpaque(), retain: nil, release: nil,
                copyDescription: nil)
            let flags = UInt32(
                kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer
                    | kFSEventStreamCreateFlagUseCFTypes)
            let callback: FSEventStreamCallback = { _, info, count, paths, _, _ in
                guard let info else { return }
                let box = Unmanaged<Box>.fromOpaque(info).takeUnretainedValue()
                let list = unsafeBitCast(paths, to: NSArray.self) as? [String] ?? []
                box.handler(Array(list.prefix(count)))
            }
            guard
                let stream = FSEventStreamCreate(
                    nil, callback, &context, [root] as CFArray, FSEventStreamEventId(kFSEventStreamEventIdSinceNow),
                    latency, flags)
            else {
                continuation.finish()
                return
            }
            let queue = DispatchQueue(label: "diffpilot.repo-watcher", qos: .utility)
            FSEventStreamSetDispatchQueue(stream, queue)
            FSEventStreamStart(stream)
            continuation.onTermination = { _ in
                FSEventStreamStop(stream)
                FSEventStreamInvalidate(stream)
                FSEventStreamRelease(stream)
                withExtendedLifetime(box) {}
            }
        }
    }

    static func summarize(_ paths: [String], root: String) -> (isRelevant: Bool, headChanged: Bool) {
        var isRelevant = false
        var headChanged = false
        for path in paths {
            guard let relative = relativePath(path, root: root), isRelevantPath(relative) else { continue }
            isRelevant = true
            if relative == ".git/HEAD" { headChanged = true }
        }
        return (isRelevant, headChanged)
    }

    static func isRelevantPath(_ relative: String) -> Bool {
        let parts = relative.split(separator: "/", omittingEmptySubsequences: true)
        guard let first = parts.first else { return true }
        if first == ".git" {
            guard parts.count > 1 else { return false }
            switch parts[1] {
            case "HEAD", "index", "packed-refs": return parts.count == 2
            case "refs": return parts.count > 2
            default: return false
            }
        }
        return !parts.contains(".git")
    }

    private static func relativePath(_ path: String, root: String) -> String? {
        if path == root { return "" }
        let prefix = root.hasSuffix("/") ? root : root + "/"
        return path.hasPrefix(prefix) ? String(path.dropFirst(prefix.count)) : nil
    }

    private final class Box: @unchecked Sendable {
        let handler: ([String]) -> Void
        init(_ handler: @escaping ([String]) -> Void) { self.handler = handler }
    }
}
