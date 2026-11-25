import Foundation
import Observation

nonisolated struct ViewedKey: Hashable, Codable, Sendable {
    var repo: String
    var branch: String

    init(repo: String, branch: String) {
        self.repo = repo
        self.branch = branch
    }

    init(repo: URL, comparison: Comparison, refs: Refs?) {
        self.init(
            repo: repo.standardizedFileURL.path, branch: comparison.branch ?? refs?.checkedOut ?? "HEAD")
    }

    fileprivate var storageKey: String { repo + "\n" + branch }
}

nonisolated final class ViewedStore: @unchecked Sendable {
    static let shared = fromEnvironment()

    let url: URL?
    private let lock = NSLock()
    private var memory: [String: [String]] = [:]
    private let seed: Set<String>

    init(url: URL? = nil, seed: Set<String> = []) {
        self.url = url
        self.seed = seed
    }

    static func defaultURL() -> URL? {
        guard
            let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
        else { return nil }
        let folder = Bundle.main.bundleIdentifier ?? "diffpilot"
        return support.appendingPathComponent(folder).appendingPathComponent("viewed.json")
    }

    static func fromEnvironment(_ environment: [String: String] = ProcessInfo.processInfo.environment) -> ViewedStore {
        guard let path = environment["DIFFPILOT_VIEWED_SEED"], !path.isEmpty else {
            return ViewedStore(url: defaultURL())
        }
        let paths = (try? Data(contentsOf: URL(fileURLWithPath: path))).flatMap {
            try? JSONDecoder().decode([String].self, from: $0)
        }
        return ViewedStore(url: nil, seed: Set(paths ?? []))
    }

    func load(_ key: ViewedKey) -> Set<String> {
        lock.lock()
        defer { lock.unlock() }
        guard let url else { return Set(memory[key.storageKey] ?? Array(seed)) }
        return Set(readFile(url)[key.storageKey] ?? [])
    }

    func save(_ key: ViewedKey, _ paths: Set<String>) {
        lock.lock()
        defer { lock.unlock() }
        guard let url else {
            memory[key.storageKey] = paths.sorted()
            return
        }
        var all = readFile(url)
        if paths.isEmpty { all[key.storageKey] = nil } else { all[key.storageKey] = paths.sorted() }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(all) else { return }
        try? FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }

    private func readFile(_ url: URL) -> [String: [String]] {
        (try? Data(contentsOf: url)).flatMap { try? JSONDecoder().decode([String: [String]].self, from: $0) } ?? [:]
    }
}

@Observable
final class ViewedFiles {
    private(set) var paths: Set<String>
    private let key: ViewedKey
    private let store: ViewedStore

    init(key: ViewedKey, store: ViewedStore = .shared) {
        self.key = key
        self.store = store
        paths = store.load(key)
    }

    func toggle(_ path: String) {
        if paths.remove(path) == nil { paths.insert(path) }
        store.save(key, paths)
    }

    func contains(_ path: String) -> Bool { paths.contains(path) }
}
