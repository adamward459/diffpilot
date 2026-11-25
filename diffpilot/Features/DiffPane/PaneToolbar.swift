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
    }
}
