import SwiftUI

struct ComparisonControl: View {
    let model: ComparisonModel
    var compact = false
    @Environment(\.appearsActive) private var appearsActive

    var body: some View {
        if let refs = model.refs, let comparison = model.comparison {
            HStack(spacing: 6) {
                Menu {
                    BranchMenu(refs: refs, comparison: comparison, select: model.select)
                } label: {
                    Label(
                        comparison.branch ?? refs.checkedOut ?? "Detached HEAD",
                        systemImage: comparison.branch == nil ? "arrow.triangle.branch" : "eye")
                }
                .help(comparison.branch == nil ? "Checked-out branch" : "Read only. This branch is not checked out.")
                .accessibilityIdentifier("comparison-branch")
                if !compact {
                    Text("compared to")
                        .foregroundStyle(appearsActive ? .secondary : .tertiary)
                }
                Menu {
                    BaseMenu(refs: refs, comparison: comparison, select: model.select)
                } label: {
                    Label(baseTitle(comparison), systemImage: baseSymbol(comparison))
                }
                .help("Compared to")
                .accessibilityIdentifier("comparison-base")
            }
            .labelStyle(.titleAndIcon)
            .fixedSize()
        }
    }

    private func baseTitle(_ comparison: Comparison) -> String {
        switch comparison {
        case .workingTree: "Working Tree Changes"
        case .staged: "Staged Changes"
        case .checkedOut(let base, false): "\(base), committed only"
        case .checkedOut(let base, true), .branch(_, let base): base
        }
    }

    private func baseSymbol(_ comparison: Comparison) -> String {
        switch comparison {
        case .workingTree: "pencil"
        case .staged: "checkmark.circle"
        case .checkedOut, .branch: "arrow.left.arrow.right"
        }
    }
}

private struct BranchMenu: View {
    let refs: Refs
    let comparison: Comparison
    let select: (Comparison) -> Void

    var body: some View {
        Section("Current") {
            CheckItem(refs.checkedOut ?? "Detached HEAD", isOn: comparison.branch == nil) {
                select(comparison.withBranch(nil, defaultBase: refs.detectedBase))
            }
        }
        branches("Local Branches", refs.local)
        branches("Remote Branches", refs.remote)
    }

    @ViewBuilder
    private func branches(_ title: String, _ names: [String]) -> some View {
        if !names.isEmpty {
            Section(title) {
                ForEach(names, id: \.self) { name in
                    CheckItem(name, isOn: comparison.branch == name) {
                        select(comparison.withBranch(name, defaultBase: refs.detectedBase))
                    }
                }
            }
        }
    }
}

private struct BaseMenu: View {
    let refs: Refs
    let comparison: Comparison
    let select: (Comparison) -> Void

    var body: some View {
        Section("Suggested") {
            ForEach(refs.suggested, id: \.self) { name in
                CheckItem(
                    name, detail: name == refs.detectedBase ? "Detected base" : nil, isOn: comparison.base == name
                ) {
                    select(comparison.withBase(name))
                }
            }
        }
        Section("Only What Is Not Committed") {
            CheckItem("Working Tree Changes Only", detail: "Like git diff HEAD", isOn: comparison == .workingTree) {
                select(.workingTree)
            }
            CheckItem("Staged Changes Only", detail: "Like git diff --staged", isOn: comparison == .staged) {
                select(.staged)
            }
        }
        .disabled(comparison.branch != nil)
        Section("Include") {
            CheckItem("Committed and Uncommitted", isOn: comparison.base != nil && comparison.includesUncommitted) {
                select(comparison.withUncommitted(true, defaultBase: refs.detectedBase))
            }
            CheckItem("Committed Only", isOn: comparison.base != nil && !comparison.includesUncommitted) {
                select(comparison.withUncommitted(false, defaultBase: refs.detectedBase))
            }
        }
        .disabled(comparison.branch != nil)
        Section("Branches, Tags and Commits") {
            refMenu("Local Branches", refs.local + [refs.checkedOut].compactMap(\.self))
            refMenu("Remote Branches", refs.remote)
            refMenu("Tags", refs.tags)
            if !refs.commits.isEmpty {
                Menu("Recent Commits") {
                    ForEach(refs.commits, id: \.self) { commit in
                        CheckItem(commit.sha, detail: commit.subject, isOn: comparison.base == commit.sha) {
                            select(comparison.withBase(commit.sha))
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func refMenu(_ title: String, _ names: [String]) -> some View {
        if !names.isEmpty {
            Menu(title) {
                ForEach(names, id: \.self) { name in
                    CheckItem(name, isOn: comparison.base == name) { select(comparison.withBase(name)) }
                }
            }
        }
    }
}

private struct CheckItem: View {
    let title: String
    var detail: String?
    let isOn: Bool
    let action: () -> Void

    init(_ title: String, detail: String? = nil, isOn: Bool, action: @escaping () -> Void) {
        self.title = title
        self.detail = detail
        self.isOn = isOn
        self.action = action
    }

    var body: some View {
        Toggle(isOn: Binding(get: { isOn }, set: { _ in action() })) {
            Text(title)
            if let detail { Text(detail) }
        }
    }
}
