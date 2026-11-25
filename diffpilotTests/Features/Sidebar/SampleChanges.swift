import Foundation

@testable import diffpilot

nonisolated enum SampleChanges {
    static func generate(count: Int) -> [FileChange] { sample(count: count).changes }

    static func sample(count: Int) -> (changes: [FileChange], viewed: Set<String>, comments: [String: Int]) {
        var viewed = Set<String>()
        var comments: [String: Int] = [:]
        var rng = SplitMix(seed: 0x9E37_79B9_7F4A_7C15)
        var seen = Set<String>()
        var result: [FileChange] = []
        result.reserveCapacity(count)

        let roots = ["src", "src", "src", "src", "tests", "tests", "docs", "packages", "scripts"]
        let areas = [
            "api", "components", "hooks", "lib", "types", "utils", "store", "features/auth", "features/inbox",
            "features/billing", "views", "models",
        ]
        let subs = ["", "", "", "inbox", "compose", "settings", "shared/forms", "legacy"]
        let words = [
            "Archive", "Message", "Inbox", "Toast", "Query", "Client", "Undo", "Thread", "Label", "Filter", "Draft",
            "Sync", "Cache", "Session", "Token", "Router", "Panel", "Dialog", "Avatar", "Search",
        ]
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
            let size = 1 + rng.next(40)
            let deletions = status == .added ? 0 : rng.next(15)
            if rng.next(100) < 25 { viewed.insert(path) }
            let isUncommitted = rng.next(100) < 12
            if rng.next(100) < 15 { comments[path] = 1 + rng.next(5) }
            result.append(
                FileChange(
                    path: path,
                    status: status,
                    additions: status == .deleted ? 0 : size,
                    deletions: deletions,
                    isUncommitted: isUncommitted
                ))
        }
        return (result.sorted { $0.path < $1.path }, viewed, comments)
    }
}

private nonisolated struct SplitMix {
    var state: UInt64

    init(seed: UInt64) { state = seed }

    mutating func next(_ bound: Int) -> Int {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return Int((z ^ (z >> 31)) % UInt64(bound))
    }
}
