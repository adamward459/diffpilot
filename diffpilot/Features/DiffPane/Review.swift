import Foundation

nonisolated enum Severity: String, Codable, Sendable, CaseIterable {
    case mustFix = "must-fix"
    case suggestion
    case nit
    case question

    var title: String {
        switch self {
        case .mustFix: "Must fix"
        case .suggestion: "Suggestion"
        case .nit: "Nit"
        case .question: "Question"
        }
    }
}

nonisolated struct Finding: Equatable, Sendable, Codable {
    var path: String
    var line: Int?
    var severity: Severity
    var title: String
    var body: String
}

nonisolated struct ReviewResult: Equatable, Sendable, Codable {
    var summary: String
    var findings: [Finding]

    var counts: [Severity: Int] {
        findings.reduce(into: [:]) { counts, finding in counts[finding.severity, default: 0] += 1 }
    }
}

nonisolated struct ReviewRequest: Equatable, Sendable {
    var repo: URL
    var revisions: [String]
    var focus: String
    var instructions: String = ""
    var paths: [String] = []
    var model = ReviewModel.standard
}

nonisolated struct ReviewModel: Equatable, Hashable, Sendable {
    enum Name: String, CaseIterable, Sendable { case opus, sonnet, haiku }
    enum Effort: String, CaseIterable, Sendable { case low, medium, high }

    static let standard = ReviewModel(name: .opus, effort: .high)

    var name: Name
    var effort: Effort

    var title: String { "\(name.rawValue.capitalized) · \(effort.rawValue.capitalized)" }
}

enum ReviewScope: Equatable {
    case all
    case openFile
}

nonisolated protocol ClaudeReviewer: Sendable {
    func review(_ request: ReviewRequest) async throws -> ReviewResult
}

enum ReviewRun: Equatable {
    case idle
    case running(since: Date)
    case done(counts: [Severity: Int])
    case failed(String)

    var label: String {
        switch self {
        case .idle: "idle"
        case .running: "running"
        case .done(let counts):
            "done:" + Severity.allCases.map { String(counts[$0, default: 0]) }.joined(separator: "/")
        case .failed: "failed"
        }
    }
}

nonisolated enum ReviewFocus {
    static let presets = ["Security", "Performance", "Correctness", "Naming", "Tests", "Types"]
    static let everything = "everything"

    static func request(selected: Set<String>, text: String) -> (focus: String, instructions: String) {
        let names = presets.filter(selected.contains)
        return (
            names.isEmpty ? everything : names.joined(separator: ", "),
            text.trimmingCharacters(in: .whitespacesAndNewlines)
        )
    }

    static func key(repo: URL) -> String { "reviewFocuses.\(repo.path)" }

    static func saved(repo: URL, defaults: UserDefaults = .standard) -> Set<String> {
        Set(defaults.stringArray(forKey: key(repo: repo)) ?? []).intersection(presets)
    }

    static func save(_ selected: Set<String>, repo: URL, defaults: UserDefaults = .standard) {
        defaults.set(presets.filter(selected.contains), forKey: key(repo: repo))
    }
}

extension ReviewPrompt {
    nonisolated static func render(_ request: ReviewRequest) -> String {
        template
            .replacingOccurrences(of: "{{revisions}}", with: diffArguments(request))
            .replacingOccurrences(of: "{{focus}}", with: request.focus)
            .replacingOccurrences(
                of: "{{instructions}}", with: instructions(request.instructions) + scope(request.paths))
    }

    nonisolated static func diffArguments(_ request: ReviewRequest) -> String {
        let revisions = request.revisions.joined(separator: " ")
        guard !request.paths.isEmpty else { return revisions }
        return revisions + " -- " + request.paths.map { "'\($0)'" }.joined(separator: " ")
    }

    nonisolated static func scope(_ paths: [String]) -> String {
        guard !paths.isEmpty else { return "" }
        let list = paths.map { "- `\($0)`" }.joined(separator: "\n")
        return
            "\n## Scope\n\nReview only these files. You may read other files for context, but do not report findings on them.\n\n\(list)\n"
    }

    nonisolated static func instructions(_ text: String) -> String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "" }
        let quoted = trimmed.split(separator: "\n", omittingEmptySubsequences: false).map { "> \($0)" }.joined(
            separator: "\n")
        return
            "\n## The user's instructions\n\nThe user asked for this review with the guidance below. It takes priority over the generic passes in Step 2: spend most of the review on it, and say in your final summary how you addressed it.\n\n\(quoted)\n"
    }
}

nonisolated struct PlannedThread: Equatable, Sendable {
    var anchor: ThreadAnchor
    var finding: FindingMeta?
    var body: String
}

nonisolated enum ReviewPlan {
    static func threads(for result: ReviewResult, files: [FileDiff]) -> [PlannedThread] {
        var planned: [PlannedThread] = []
        let summary = result.summary.trimmingCharacters(in: .whitespacesAndNewlines)
        if !summary.isEmpty { planned.append(PlannedThread(anchor: .change, finding: nil, body: summary)) }
        for finding in result.findings {
            let (anchor, location) = place(finding, in: files)
            planned.append(
                PlannedThread(
                    anchor: anchor,
                    finding: FindingMeta(severity: finding.severity, title: finding.title, location: location),
                    body: finding.body))
        }
        return planned
    }

    private static func place(_ finding: Finding, in files: [FileDiff]) -> (ThreadAnchor, String?) {
        var path = finding.path
        if path.hasPrefix("./") { path.removeFirst(2) }
        guard let file = files.first(where: { $0.path == path }) else {
            let place = finding.line.map { "\(path):\($0)" } ?? path
            return (.change, place)
        }
        guard let line = finding.line else { return (.file(path: path), nil) }
        if file.hunks.contains(where: { $0.lines.contains { $0.new == line } }) {
            return (.line(path: path, side: .new, line: line), nil)
        }
        return (.file(path: path), "Line \(line)")
    }
}
