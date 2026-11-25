import Foundation
import Observation

@Observable
final class SidebarModel {
    var root = FileNode.empty
    var files: [FileNode] = []
    var summary = ChangeSummary()
    var filter = FileFilter()
    var mode = SidebarMode.tree
    var selection: String?
    let repo: URL
    var isLoading = true

    init(repo: URL) {
        self.repo = repo
    }

    func load() async {
        let repo = repo
        let loaded = await Task.detached(priority: .userInitiated) {
            let changes = (try? RepoChanges.load(repo: repo)) ?? []
            let root = FileTree.build(changes)
            return (root, FileTree.files(in: root), ChangeSummary(changes))
        }.value
        (root, files, summary) = loaded
        isLoading = false
    }
}
