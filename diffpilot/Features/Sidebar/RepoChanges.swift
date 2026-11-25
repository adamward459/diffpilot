import Foundation

nonisolated struct ReviewState: Decodable, Equatable {
    var viewed: Set<String> = []
    var comments: [String: Int] = [:]
}

nonisolated enum RepoChanges {
    static func load(repo: URL, comparison: Comparison) throws -> [FileChange] {
        let review =
            (try? Data(contentsOf: repo.appendingPathComponent(".git/diffpilot-review.json")))
            .flatMap { try? JSONDecoder().decode(ReviewState.self, from: $0) } ?? ReviewState()
        return parse(
            nameStatus: try Git.run(repo, ["diff", "-z", "-M", "--name-status"] + comparison.revisions),
            numstat: try Git.run(repo, ["diff", "-z", "-M", "--numstat"] + comparison.revisions),
            uncommitted: comparison.includesUncommitted
                ? try Git.run(repo, ["diff", "-z", "--name-only", "HEAD"]) : "",
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
}
