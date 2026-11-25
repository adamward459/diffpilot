import Foundation

nonisolated enum RecentFolder {
    static let key = "recentFolderBookmark"

    static func restore(
        defaults: UserDefaults = .standard, environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> URL? {
        if let path = environment["DIFFPILOT_REPO"], !path.isEmpty { return URL(fileURLWithPath: path) }
        guard let data = defaults.data(forKey: key), !data.isEmpty else { return nil }
        var isStale = false
        guard
            let url = try? URL(
                resolvingBookmarkData: data, options: [], relativeTo: nil,
                bookmarkDataIsStale: &isStale)
        else { return nil }
        if isStale { remember(url, defaults: defaults) }
        return url
    }

    static func remember(_ url: URL, defaults: UserDefaults = .standard) {
        let data = try? url.bookmarkData(
            options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
        defaults.set(data, forKey: key)
    }
}
