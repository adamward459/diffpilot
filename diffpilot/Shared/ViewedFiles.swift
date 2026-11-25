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
    private var memory: [String: [String: String]] = [:]
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

    func load(_ key: ViewedKey) -> [String: String] {
        lock.lock()
        defer { lock.unlock() }
        guard let url else {
            return memory[key.storageKey] ?? Dictionary(uniqueKeysWithValues: seed.map { ($0, ViewedFiles.unknown) })
        }
        return readFile(url)[key.storageKey] ?? [:]
    }

    func save(_ key: ViewedKey, _ paths: [String: String]) {
        lock.lock()
        defer { lock.unlock() }
        guard let url else {
            memory[key.storageKey] = paths
            return
        }
        var all = readFile(url)
        all[key.storageKey] = paths.isEmpty ? nil : paths
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(all) else { return }
        try? FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }

    private func readFile(_ url: URL) -> [String: [String: String]] {
        (try? Data(contentsOf: url)).flatMap { try? JSONDecoder().decode([String: [String: String]].self, from: $0) }
            ?? [:]
    }
}

@Observable
final class ViewedFiles {
    static let unknown = ""

    private(set) var fingerprints: [String: String]
    private let key: ViewedKey
    private let store: ViewedStore

    init(key: ViewedKey, store: ViewedStore = .shared) {
        self.key = key
        self.store = store
        fingerprints = store.load(key)
    }

    var paths: Set<String> { Set(fingerprints.keys) }

    func contains(_ path: String) -> Bool { fingerprints[path] != nil }

    func toggle(_ path: String, fingerprint: String = unknown) {
        if fingerprints.removeValue(forKey: path) == nil { fingerprints[path] = fingerprint }
        store.save(key, fingerprints)
    }

    func clearChanged(_ current: [String: String]) -> [String] {
        var next = fingerprints
        var cleared: [String] = []
        for (path, saved) in fingerprints {
            guard let now = current[path], saved != now else { continue }
            if saved == Self.unknown {
                next[path] = now
            } else {
                next[path] = nil
                cleared.append(path)
            }
        }
        guard next != fingerprints else { return [] }
        fingerprints = next
        store.save(key, next)
        return cleared.sorted()
    }
}
