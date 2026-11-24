import SwiftUI

struct FileSidebar: View {
    @Bindable var model: SidebarModel

    var body: some View {
        VStack(spacing: 0) {
            SummaryHeader(summary: model.summary, isLoading: model.isLoading)
            FilterRow(model: model)
            FileOutline(
                root: model.root, files: model.files, filter: model.filter, mode: model.mode,
                selection: $model.selection)
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
                    Text("\(summary.viewed) of \(summary.fileCount) viewed")
                }
            }
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .minimumScaleFactor(0.8)

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
        HStack(spacing: 6) {
            HStack(spacing: 6) {
                Image(systemName: "line.3.horizontal.decrease")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                TextField("Filter", text: $model.filter.query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 12))
            }
            .padding(.horizontal, 7)
            .frame(height: 24)
            .background(.quaternary.opacity(0.6), in: RoundedRectangle(cornerRadius: 6))

            ToggleButton(symbol: "pencil", help: "Only uncommitted", isOn: $model.filter.onlyUncommitted)
            ToggleButton(symbol: "eye", help: "Hide viewed", isOn: $model.filter.hideViewed)
            Button {
                model.mode = model.mode == .tree ? .list : .tree
            } label: {
                Image(systemName: model.mode == .tree ? "list.bullet.indent" : "list.bullet")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .frame(width: 24, height: 24)
            }
            .buttonStyle(.plain)
            .help(model.mode == .tree ? "Switch to list" : "Switch to tree")
        }
        .padding(.horizontal, 10)
        .frame(height: 38)
        .overlay(alignment: .top) { Divider() }
        .overlay(alignment: .bottom) { Divider() }
    }
}

private struct ToggleButton: View {
    let symbol: String
    let help: String
    @Binding var isOn: Bool

    var body: some View {
        Button {
            isOn.toggle()
        } label: {
            Image(systemName: symbol)
                .font(.system(size: 12))
                .foregroundStyle(isOn ? Color.accentColor : .secondary)
                .frame(width: 24, height: 24)
                .background(isOn ? Color.accentColor.opacity(0.15) : .clear, in: RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
        .help(help)
    }
}
