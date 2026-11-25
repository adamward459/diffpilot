import SwiftUI

struct FileSidebar: View {
    @Bindable var model: SidebarModel
    let comments: [String: Int]
    let toggleViewed: (String) -> Void

    var body: some View {
        VStack(spacing: 0) {
            SummaryHeader(summary: model.summary, isLoading: model.isLoading)
            FilterRow(model: model)
            FileOutline(
                root: model.root, files: model.files, filter: model.filter, mode: model.mode,
                marks: FileMarks(viewed: model.viewed.paths, comments: comments),
                toggleViewed: toggleViewed, selection: $model.selection)
        }
    }
}

private struct SummaryHeader: View {
    let summary: ChangeSummary
    let isLoading: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 5) {
                if isLoading {
                    Text("Loading changes…")
                } else {
                    Text("\(summary.fileCount) files")
                    Text("·")
                    HStack(spacing: 4) {
                        Text("+\(summary.additions.formatted(.number.notation(.compactName)))").foregroundStyle(.green)
                        Text("−\(summary.deletions.formatted(.number.notation(.compactName)))").foregroundStyle(.red)
                    }
                    .font(.system(size: 11.5, design: .monospaced))
                    Text("·")
                    Text("\(summary.viewed)/\(summary.fileCount) viewed")
                }
            }
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .minimumScaleFactor(0.8)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("summary")

            Capsule()
                .fill(.quaternary)
                .frame(height: 4)
                .overlay(alignment: .leading) {
                    GeometryReader { proxy in
                        Capsule()
                            .fill(Color.accentColor)
                            .frame(width: proxy.size.width * viewedFraction)
                    }
                }
        }
        .padding(.horizontal, 12)
        .padding(.top, 2)
        .padding(.bottom, 10)
    }

    private var viewedFraction: Double {
        summary.fileCount == 0 ? 0 : Double(summary.viewed) / Double(summary.fileCount)
    }
}

private struct FilterRow: View {
    @Bindable var model: SidebarModel

    var body: some View {
        GlassEffectContainer(spacing: 6) {
            HStack(spacing: 6) {
                HStack(spacing: 6) {
                    Image(systemName: "line.3.horizontal.decrease")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                    TextField("Filter", text: $model.filter.query)
                        .textFieldStyle(.plain)
                        .font(.system(size: 12))
                        .accessibilityIdentifier("filter-field")
                }
                .padding(.horizontal, 10)
                .frame(height: 28)
                .glassEffect(.regular, in: .capsule)

                ToggleButton(
                    symbol: "pencil", title: "Only uncommitted", id: "toggle-uncommitted",
                    isOn: $model.filter.onlyUncommitted)
                ToggleButton(
                    symbol: "eye", title: "Hide viewed", id: "toggle-hide-viewed", isOn: $model.filter.hideViewed)
                Button {
                    model.mode = model.mode == .tree ? .list : .tree
                } label: {
                    Image(systemName: model.mode == .tree ? "list.bullet.indent" : "list.bullet")
                        .font(.system(size: 12))
                        .frame(width: 20, height: 20)
                }
                .buttonStyle(.glass)
                .buttonBorderShape(.circle)
                .help(model.mode == .tree ? "Switch to list" : "Switch to tree")
                .accessibilityLabel(model.mode == .tree ? "Switch to list" : "Switch to tree")
                .accessibilityIdentifier("mode-button")
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
    }
}

private struct ToggleButton: View {
    let symbol: String
    let title: String
    let id: String
    @Binding var isOn: Bool

    var body: some View {
        Button {
            isOn.toggle()
        } label: {
            Image(systemName: symbol)
                .font(.system(size: 12))
                .frame(width: 20, height: 20)
        }
        .buttonStyle(.glass(isOn ? .regular.tint(.accentColor) : .regular))
        .buttonBorderShape(.circle)
        .help(title)
        .accessibilityLabel(title)
        .accessibilityValue(isOn ? "on" : "off")
        .accessibilityIdentifier(id)
    }
}
