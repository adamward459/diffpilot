import Observation

@Observable
final class SidebarModel {
    var root = FileNode.empty
    var files: [FileNode] = []
    var summary = ChangeSummary()
    var filter = FileFilter()
    var mode = SidebarMode.tree
    var selection: String?
    var isLoading = true

    func load() async {
        let loaded = await Task.detached(priority: .userInitiated) {
            let changes = SampleChanges.load()
            let root = FileTree.build(changes)
            return (root, FileTree.files(in: root), ChangeSummary(changes))
        }.value
        (root, files, summary) = loaded
        isLoading = false
    }
}
