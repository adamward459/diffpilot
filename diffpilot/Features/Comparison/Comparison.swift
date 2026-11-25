import Foundation

nonisolated enum Comparison: Codable, Hashable, Sendable {
    case checkedOut(base: String, includesUncommitted: Bool)
    case workingTree
    case staged
    case branch(String, base: String)

    var revisions: [String] {
        switch self {
        case .checkedOut(let base, true): ["--merge-base", base]
        case .checkedOut(let base, false): ["--merge-base", base, "HEAD"]
        case .workingTree: ["HEAD"]
        case .staged: ["--cached"]
        case .branch(let branch, let base): ["--merge-base", base, branch]
        }
    }

    var base: String? {
        switch self {
        case .checkedOut(let base, _), .branch(_, let base): base
        case .workingTree, .staged: nil
        }
    }

    var branch: String? {
        if case .branch(let branch, _) = self { branch } else { nil }
    }

    var includesUncommitted: Bool {
        switch self {
        case .checkedOut(_, let includes): includes
        case .workingTree, .staged: true
        case .branch: false
        }
    }

    var refs: [String] { [base, branch].compactMap(\.self) }

    func withBranch(_ branch: String?, defaultBase: String) -> Comparison {
        let base = base ?? defaultBase
        guard let branch else { return .checkedOut(base: base, includesUncommitted: true) }
        return .branch(branch, base: base)
    }

    func withBase(_ base: String) -> Comparison {
        switch self {
        case .branch(let branch, _): .branch(branch, base: base)
        case .checkedOut(_, let includes): .checkedOut(base: base, includesUncommitted: includes)
        case .workingTree, .staged: .checkedOut(base: base, includesUncommitted: true)
        }
    }

    func withUncommitted(_ includes: Bool, defaultBase: String) -> Comparison {
        if branch != nil { return self }
        return .checkedOut(base: base ?? defaultBase, includesUncommitted: includes)
    }
}
