import Foundation

nonisolated struct Refs: Equatable, Sendable {
    struct Commit: Hashable, Sendable {
        var sha: String
        var subject: String
    }

    var checkedOut: String?
    var detectedBase: String
    var local: [String] = []
    var remote: [String] = []
    var tags: [String] = []
    var commits: [Commit] = []

    var suggested: [String] {
        let short = detectedBase.split(separator: "/", maxSplits: 1).last.map(String.init)
        let localTwin = short.flatMap { $0 != detectedBase && local.contains($0) ? $0 : nil }
        return [detectedBase] + [localTwin].compactMap(\.self)
    }

    func contains(_ ref: String) -> Bool {
        local.contains(ref) || remote.contains(ref) || tags.contains(ref) || commits.contains { $0.sha == ref }
    }

    static func load(repo: URL) -> Refs {
        parse(
            refs: (try? Git.run(
                repo,
                [
                    "for-each-ref", "--sort=-committerdate", "--format=%(refname)", "refs/heads", "refs/remotes",
                    "refs/tags",
                ])) ?? "",
            head: (try? Git.run(repo, ["symbolic-ref", "-q", "--short", "HEAD"])) ?? "",
            originHead: (try? Git.run(repo, ["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"])) ?? "",
            log: (try? Git.run(repo, ["log", "-n", "10", "--format=%h%x09%s"])) ?? "")
    }

    static func parse(refs: String, head: String, originHead: String, log: String) -> Refs {
        var local: [String] = []
        var remote: [String] = []
        var tags: [String] = []
        for line in refs.split(separator: "\n") {
            if let name = line.dropPrefix("refs/heads/") {
                local.append(name)
            } else if let name = line.dropPrefix("refs/remotes/"), !name.hasSuffix("/HEAD") {
                remote.append(name)
            } else if let name = line.dropPrefix("refs/tags/") {
                tags.append(name)
            }
        }
        let commits = log.split(separator: "\n").compactMap { line -> Commit? in
            let parts = line.split(separator: "\t", maxSplits: 1, omittingEmptySubsequences: false)
            guard let sha = parts.first, !sha.isEmpty else { return nil }
            return Commit(sha: String(sha), subject: parts.count > 1 ? String(parts[1]) : "")
        }
        let checkedOut = trimmed(head)
        let detectedBase =
            trimmed(originHead) ?? ["main", "master"].first(where: local.contains)
            ?? local.first(where: { $0 != checkedOut }) ?? "HEAD"
        return Refs(
            checkedOut: checkedOut, detectedBase: detectedBase,
            local: local.filter { $0 != checkedOut }, remote: remote, tags: tags, commits: commits)
    }

    private static func trimmed(_ text: String) -> String? {
        let value = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return value.isEmpty ? nil : value
    }
}

extension Substring {
    fileprivate nonisolated func dropPrefix(_ prefix: String) -> String? {
        hasPrefix(prefix) ? String(dropFirst(prefix.count)) : nil
    }
}
