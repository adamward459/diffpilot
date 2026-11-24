import SwiftUI

@main
struct diffpilotApp: App {
    init() {
        switch ProcessInfo.processInfo.environment["DIFFPILOT_APPEARANCE"] {
        case "light": NSApplication.shared.appearance = NSAppearance(named: .aqua)
        case "dark": NSApplication.shared.appearance = NSAppearance(named: .darkAqua)
        default: break
        }
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
        .defaultSize(width: 1100, height: 700)
    }
}
