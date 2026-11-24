import SwiftUI

struct ContentView: View {
    @State private var model = SidebarModel()

    var body: some View {
        NavigationSplitView {
            FileSidebar(model: model)
                .navigationSplitViewColumnWidth(min: 240, ideal: 272, max: 480)
        } detail: {
            if let path = model.selection {
                Text(path)
                    .font(.system(.title3, design: .monospaced))
                    .textSelection(.enabled)
                    .accessibilityIdentifier("detail-path")
            } else {
                Text("Select a file")
                    .foregroundStyle(.secondary)
            }
        }
        .task { await model.load() }
    }
}
