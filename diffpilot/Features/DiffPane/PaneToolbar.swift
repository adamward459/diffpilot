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
            ReviewControl(model: model)
        }
    }
}
