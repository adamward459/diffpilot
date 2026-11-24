import Foundation

nonisolated enum SampleChanges {
    static let defaultCount = 2000

    static func configuredCount() -> Int {
        ProcessInfo.processInfo.environment["DIFFPILOT_SAMPLE_COUNT"].flatMap { Int($0) } ?? defaultCount
    }

    static func load() -> [FileChange] {
        if ProcessInfo.processInfo.environment["DIFFPILOT_FIXTURE"] == "design" { return design }
        return generate(count: configuredCount())
    }

    static let design: [FileChange] = {
        func file(
            _ path: String, _ status: FileStatus, _ add: Int, _ del: Int, viewed: Bool = false,
            uncommitted: Bool = false, comments: Int = 0
        ) -> FileChange {
            FileChange(
                path: path, status: status, additions: add, deletions: del, isViewed: viewed,
                isUncommitted: uncommitted, commentCount: comments)
        }
        let files: [FileChange] = [
            file("docs/archive.md", .added, 19, 0, viewed: true),
            file("src/api/archive.ts", .modified, 38, 6, comments: 1),
            file("src/api/client.ts", .modified, 4, 1, viewed: true),
            file("src/components/inbox/ArchiveToast.tsx", .added, 48, 0),
            file("src/components/inbox/MessageRow.tsx", .modified, 23, 15),
            file("src/hooks/useArchiveUndo.ts", .added, 64, 0, uncommitted: true, comments: 2),
            file("src/lib/queryKeys.ts", .modified, 3, 1, viewed: true),
            file("src/types/message.ts", .modified, 5, 0),
            file("tests/api/archive.test.ts", .modified, 31, 4, uncommitted: true),
            file("tests/hooks/useArchiveUndo.test.ts", .added, 112, 0),
            file("package.json", .modified, 1, 1),
            file("pnpm-lock.yaml", .modified, 21, 8),
        ]
        return files.sorted { $0.path < $1.path }
    }()

    static func generate(count: Int) -> [FileChange] {
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
            result.append(
                FileChange(
                    path: path,
                    status: status,
                    additions: status == .deleted ? 0 : size,
                    deletions: status == .added ? 0 : rng.next(15),
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
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return Int((z ^ (z >> 31)) % UInt64(bound))
    }
}
