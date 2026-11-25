import Foundation
import Observation

@Observable
final class SidebarModel {
    var root = FileNode.empty
    var files: [FileNode] = []
    var filter = FileFilter()
    var mode = SidebarMode.tree
    var selection: String?
    let repo: URL
    let comparison: Comparison
    let viewed: ViewedFiles
    var isLoading = true
    private var changes: [FileChange] = []

    init(repo: URL, comparison: Comparison, viewed: ViewedFiles) {
        self.repo = repo
        self.comparison = comparison
        self.viewed = viewed
    }

    var summary: ChangeSummary { ChangeSummary(changes, viewed: viewed.paths) }

    func load() async {
        let repo = repo
        let comparison = comparison
        let loaded = await Task.detached(priority: .userInitiated) {
            let changes = (try? RepoChanges.load(repo: repo, comparison: comparison)) ?? []
            let root = FileTree.build(changes)
            return (changes, root, FileTree.files(in: root))
        }.value
        (changes, root, files) = loaded
        isLoading = false
    }
}
