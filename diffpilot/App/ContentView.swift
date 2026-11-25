import SwiftUI

struct ContentView: View {
    @State private var model = SidebarModel()
    @State private var diff = DiffPaneModel(repo: RepoChanges.configuredRepo())

    var body: some View {
        NavigationSplitView {
            FileSidebar(model: model)
                .navigationSplitViewColumnWidth(min: 240, ideal: 272, max: 480)
        } detail: {
            DiffPaneView(model: diff)
        }
        .onChange(of: model.selection) { _, path in
            if let path { diff.reveal(path) }
        }
        .task { await model.load() }
        .task { await diff.load() }
    }
}
