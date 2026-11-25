import SwiftUI

struct PaneToolbar: ToolbarContent {
    @Bindable var model: DiffPaneModel

    var body: some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            Picker("Mode", selection: $model.mode) {
                ForEach(PaneMode.allCases, id: \.self) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier("pane-mode")
        }
        ToolbarItem(placement: .primaryAction) {
            Button {
                model.commentOnVisibleFile()
            } label: {
                Label("Comment on file", systemImage: "plus.bubble")
            }
            .help("Comment on file")
            .disabled(model.visiblePath == nil)
            .accessibilityIdentifier("comment-on-file")
        }
        ToolbarItem(placement: .primaryAction) {
            Toggle(isOn: viewed) {
                Label("Viewed", systemImage: "checkmark.circle")
            }
            .toggleStyle(.button)
            .help("Mark file as viewed")
            .disabled(model.visiblePath == nil)
            .accessibilityIdentifier("viewed-toggle")
        }
    }

    private var viewed: Binding<Bool> {
        Binding(
            get: { model.visiblePath.map(model.viewed.contains) ?? false },
            set: { _ in if let path = model.visiblePath { model.toggleViewed(path) } })
    }
}
