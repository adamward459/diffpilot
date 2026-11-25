import Foundation

nonisolated enum ComparisonStore {
    static let key = "comparisons"

    static func load(repo: URL, checkedOut: String?, defaults: UserDefaults = .standard) -> Comparison? {
        guard let saved = defaults.dictionary(forKey: key)?[entry(repo: repo, checkedOut: checkedOut)] as? String
        else { return nil }
        return try? JSONDecoder().decode(Comparison.self, from: Data(saved.utf8))
    }

    static func save(
        _ comparison: Comparison, repo: URL, checkedOut: String?, defaults: UserDefaults = .standard
    ) {
        guard let data = try? JSONEncoder().encode(comparison) else { return }
        var all = defaults.dictionary(forKey: key) ?? [:]
        all[entry(repo: repo, checkedOut: checkedOut)] = String(decoding: data, as: UTF8.self)
        defaults.set(all, forKey: key)
    }

    static func entry(repo: URL, checkedOut: String?) -> String {
        "\(repo.standardizedFileURL.path)\n\(checkedOut ?? "HEAD")"
    }
}
