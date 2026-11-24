import Foundation

nonisolated struct ReviewState: Decodable, Equatable {
    var viewed: Set<String> = []
    var comments: [String: Int] = [:]
}

nonisolated enum RepoChanges {
    enum LoadError: Error {
        case gitNotFound
        case gitFailed(String)
    }

    // /usr/bin/git is an xcrun shim, and xcrun refuses to run inside the App Sandbox.
    static let gitCandidates = [
        "/Applications/Xcode.app/Contents/Developer/usr/bin/git",
        "/Library/Developer/CommandLineTools/usr/bin/git",
        "/opt/homebrew/bin/git",
        "/usr/local/bin/git",
    ]

    static func configuredRepo() -> URL? {
        guard let value = ProcessInfo.processInfo.environment["DIFFPILOT_REPO"], !value.isEmpty else { return nil }
        if value.hasPrefix("/") { return URL(fileURLWithPath: value) }
        return URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("fixtures").appendingPathComponent(value)
    }

    static func load(repo: URL, base: String = "main") throws -> [FileChange] {
        let review =
            (try? Data(contentsOf: repo.appendingPathComponent(".git/diffpilot-review.json")))
            .flatMap { try? JSONDecoder().decode(ReviewState.self, from: $0) } ?? ReviewState()
        return parse(
            nameStatus: try git(repo, "diff", "-z", "-M", "--name-status", base),
            numstat: try git(repo, "diff", "-z", "-M", "--numstat", base),
            uncommitted: try git(repo, "diff", "-z", "--name-only", "HEAD"),
            review: review)
    }

    static func parse(nameStatus: String, numstat: String, uncommitted: String, review: ReviewState) -> [FileChange] {
        var statuses: [String: FileStatus] = [:]
        var fields = nameStatus.split(separator: "\0", omittingEmptySubsequences: false).makeIterator()
        while let code = fields.next(), !code.isEmpty {
            let isRename = code.hasPrefix("R")
            if isRename { _ = fields.next() }
            guard let path = fields.next() else { break }
            statuses[String(path)] = isRename ? .renamed : FileStatus(rawValue: String(code.prefix(1))) ?? .modified
        }

        var counts: [String: (Int, Int)] = [:]
        var entries = numstat.split(separator: "\0", omittingEmptySubsequences: false).makeIterator()
        while let entry = entries.next(), !entry.isEmpty {
            let parts = entry.split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false)
            guard parts.count == 3 else { break }
            var path = String(parts[2])
            if path.isEmpty {
                _ = entries.next()
                path = entries.next().map(String.init) ?? ""
            }
            counts[path] = (Int(parts[0]) ?? 0, Int(parts[1]) ?? 0)
        }

        let uncommittedPaths = Set(uncommitted.split(separator: "\0").map(String.init))
        return statuses.map { path, status in
            let (additions, deletions) = counts[path] ?? (0, 0)
            return FileChange(
                path: path, status: status, additions: additions, deletions: deletions,
                isViewed: review.viewed.contains(path), isUncommitted: uncommittedPaths.contains(path),
                commentCount: review.comments[path] ?? 0)
        }
        .sorted { $0.path < $1.path }
    }

    private static func git(_ repo: URL, _ args: String...) throws -> String {
        guard let binary = gitCandidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
            throw LoadError.gitNotFound
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = ["-C", repo.path] + args
        let output = Pipe()
        let errors = Pipe()
        process.standardOutput = output
        process.standardError = errors
        try process.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            let message = String(data: errors.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            throw LoadError.gitFailed(message)
        }
        return String(data: data, encoding: .utf8) ?? ""
    }
}
