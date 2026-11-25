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
    @State private var picker: ComparisonModel
    @State private var session: RepoSession?

    init(repo: URL) {
        _picker = State(initialValue: ComparisonModel(repo: repo))
    }

    var body: some View {
        NavigationSplitView {
            Group {
                if let session {
                    FileSidebar(model: session.sidebar).id(session.comparison)
                } else {
                    ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .navigationSplitViewColumnWidth(min: 240, ideal: 272, max: 480)
        } detail: {
            if let session {
                DiffPaneView(model: session.diff)
                    .id(session.comparison)
                    .toolbar { if session.diff.firstPaintAt != nil { PaneToolbar(model: session.diff) } }
            }
        }
        .toolbar {
            ToolbarItem(placement: .principal) { ComparisonControl(model: picker) }
        }
        .onChange(of: session?.sidebar.selection) { _, path in
            if let path { session?.diff.reveal(path) }
        }
        .task { await picker.load() }
        .task(id: picker.comparison) {
            guard let comparison = picker.comparison else { return }
            let next = RepoSession(repo: picker.repo, comparison: comparison)
            session = next
            async let sidebar: Void = next.sidebar.load()
            async let diff: Void = next.diff.load()
            _ = await (sidebar, diff)
        }
    }
}

private struct RepoSession {
    let comparison: Comparison
    let sidebar: SidebarModel
    let diff: DiffPaneModel

    init(repo: URL, comparison: Comparison) {
        self.comparison = comparison
        sidebar = SidebarModel(repo: repo, comparison: comparison)
        diff = DiffPaneModel(repo: repo, comparison: comparison)
    }
}
