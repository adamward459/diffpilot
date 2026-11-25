import SwiftUI

@main
struct diffpilotApp: App {
    @State private var workspace = Workspace()

    init() {
        // A session that quit with no window open would otherwise relaunch with no window.
        UserDefaults.standard.register(defaults: ["ApplePersistenceIgnoreState": true])
        switch ProcessInfo.processInfo.environment["DIFFPILOT_APPEARANCE"] {
        case "light": NSApplication.shared.appearance = NSAppearance(named: .aqua)
        case "dark": NSApplication.shared.appearance = NSAppearance(named: .darkAqua)
        default: break
        }
    }

    var body: some Scene {
        WindowGroup {
            ContentView(workspace: workspace)
        }
        .defaultSize(width: 1100, height: 700)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("Open Folder…") { workspace.openFolder() }
                    .keyboardShortcut("o")
            }
        }
    }
}
