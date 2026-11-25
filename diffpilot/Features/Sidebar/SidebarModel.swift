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
    let comparison: Comparison
    var isLoading = true

    init(repo: URL, comparison: Comparison) {
        self.repo = repo
        self.comparison = comparison
    }

    func load() async {
        let repo = repo
        let comparison = comparison
        let loaded = await Task.detached(priority: .userInitiated) {
            let changes = (try? RepoChanges.load(repo: repo, comparison: comparison)) ?? []
            let root = FileTree.build(changes)
            return (root, FileTree.files(in: root), ChangeSummary(changes))
        }.value
        (root, files, summary) = loaded
        isLoading = false
    }
}
