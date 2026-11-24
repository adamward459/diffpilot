import AppKit
import SwiftUI

struct MetricsProbe: NSViewRepresentable {
    static var isEnabled: Bool { ProcessInfo.processInfo.environment["DIFFPILOT_METRICS"] == "1" }

    func makeNSView(context: Context) -> NSView { ProbeView() }

    func updateNSView(_ view: NSView, context: Context) {}

    private final class ProbeView: NSView {
        override func isAccessibilityElement() -> Bool { true }
        override func accessibilityRole() -> NSAccessibility.Role? { .staticText }
        override func accessibilityIdentifier() -> String { "metrics" }
        override func accessibilityValue() -> Any? {
            let data = (try? JSONEncoder().encode(SidebarMetrics.snapshot)) ?? Data()
            return String(decoding: data, as: UTF8.self)
        }
    }
}
