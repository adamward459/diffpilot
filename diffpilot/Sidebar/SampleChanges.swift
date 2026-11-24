import Foundation

nonisolated enum SampleChanges {
    static let defaultCount = 2000

    static func configuredCount() -> Int {
        ProcessInfo.processInfo.environment["DIFFPILOT_SAMPLE_COUNT"].flatMap { Int($0) } ?? defaultCount
    }

    static func generate(count: Int) -> [FileChange] {
        var rng = SplitMix(seed: 0x9E3779B97F4A7C15)
        var seen = Set<String>()
        var result: [FileChange] = []
        result.reserveCapacity(count)

        let roots = ["src", "src", "src", "src", "tests", "tests", "docs", "packages", "scripts"]
        let areas = ["api", "components", "hooks", "lib", "types", "utils", "store", "features/auth", "features/inbox", "features/billing", "views", "models"]
        let subs = ["", "", "", "inbox", "compose", "settings", "shared/forms", "legacy"]
        let words = ["Archive", "Message", "Inbox", "Toast", "Query", "Client", "Undo", "Thread", "Label", "Filter", "Draft", "Sync", "Cache", "Session", "Token", "Router", "Panel", "Dialog", "Avatar", "Search"]
        let kinds = ["", "Row", "List", "View", "Provider", "Store", "Service", "Helper", "Config"]
        let extensions = ["ts", "ts", "tsx", "tsx", "swift", "swift", "md", "json", "yaml"]

        while result.count < count {
            let root = roots[rng.next(roots.count)]
            var parts = [root]
            if root == "docs" {
                if rng.next(3) == 0 { parts.append(areas[rng.next(areas.count)]) }
            } else {
                parts.append(areas[rng.next(areas.count)])
                let sub = subs[rng.next(subs.count)]
                if !sub.isEmpty { parts.append(sub) }
            }
            let ext = root == "docs" ? "md" : extensions[rng.next(extensions.count)]
            var base = words[rng.next(words.count)] + kinds[rng.next(kinds.count)]
            if ext == "ts" && rng.next(3) == 0 { base = "use" + base }
            if root == "tests" && ext != "md" && ext != "json" && ext != "yaml" { base += ".test" }
            var path = (parts + [base + "." + ext]).joined(separator: "/")
            var suffix = 2
            while !seen.insert(path).inserted {
                path = (parts + [base + "\(suffix)." + ext]).joined(separator: "/")
                suffix += 1
            }

            let roll = rng.next(100)
            let status: FileStatus = roll < 60 ? .modified : roll < 82 ? .added : roll < 90 ? .deleted : .renamed
            let size = 1 + rng.next(120)
            result.append(FileChange(
                path: path,
                status: status,
                additions: status == .deleted ? 0 : size,
                deletions: status == .added ? 0 : rng.next(40),
                isViewed: rng.next(100) < 25,
                isUncommitted: rng.next(100) < 12,
                commentCount: rng.next(100) < 15 ? 1 + rng.next(5) : 0
            ))
        }
        return result.sorted { $0.path < $1.path }
    }
}

private nonisolated struct SplitMix {
    var state: UInt64

    init(seed: UInt64) { state = seed }

    mutating func next(_ bound: Int) -> Int {
        state &+= 0x9E3779B97F4A7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58476D1CE4E5B9
        z = (z ^ (z >> 27)) &* 0x94D049BB133111EB
        return Int((z ^ (z >> 31)) % UInt64(bound))
    }
}
