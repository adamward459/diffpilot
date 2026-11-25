import AppKit
import Observation

@Observable
final class Workspace {
    private(set) var folder: URL? = RecentFolder.restore()

    func openFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        folder?.stopAccessingSecurityScopedResource()
        RecentFolder.remember(url)
        folder = url
    }
}
