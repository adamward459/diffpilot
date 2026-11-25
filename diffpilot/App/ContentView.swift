import SwiftUI

struct ContentView: View {
    let workspace: Workspace

    var body: some View {
        if let folder = workspace.folder {
            RepoView(repo: folder)
                .id(folder)
                .navigationTitle(folder.lastPathComponent)
        } else {
            NavigationSplitView {
                ContentUnavailableView {
                    Label("No Folder", systemImage: "folder")
                } description: {
                    Text("Press ⌘O to open a Git repository.")
                } actions: {
                    Button("Open Folder…") { workspace.openFolder() }
                }
                .navigationSplitViewColumnWidth(min: 240, ideal: 272, max: 480)
            } detail: {
                Color.clear
            }
        }
    }
}

private struct RepoView: View {
    @State private var model: SidebarModel
    @State private var diff: DiffPaneModel

    init(repo: URL) {
        _model = State(initialValue: SidebarModel(repo: repo))
        _diff = State(initialValue: DiffPaneModel(repo: repo))
    }

    var body: some View {
        NavigationSplitView {
            FileSidebar(model: model)
                .navigationSplitViewColumnWidth(min: 240, ideal: 272, max: 480)
        } detail: {
            DiffPaneView(model: diff)
                .toolbar { if diff.firstPaintAt != nil { PaneToolbar(model: diff) } }
        }
        .onChange(of: model.selection) { _, path in
            if let path { diff.reveal(path) }
        }
        .task { await model.load() }
        .task { await diff.load() }
    }
}
