import AppKit

nonisolated struct PaneTheme: Equatable, Sendable, Encodable {
    var colors: [String: String]
    var isDark: Bool
    var increaseContrast: Bool
    var reduceTransparency: Bool
}

extension PaneTheme {
    private static let palette: [(variable: String, color: NSColor)] = [
        ("--text", .labelColor),
        ("--text2", .secondaryLabelColor),
        ("--muted", .secondaryLabelColor),
        ("--faint", .tertiaryLabelColor),
        ("--line", .separatorColor),
        ("--line2", .gridColor),
        ("--accent", .controlAccentColor),
        ("--on-accent", .alternateSelectedControlTextColor),
        ("--ed", .textBackgroundColor),
        ("--ed2", .windowBackgroundColor),
        ("--raised", .controlBackgroundColor),
        ("--ok", .systemGreen),
        ("--err", .systemRed),
        ("--amber", .systemOrange),
    ]

    @MainActor
    static func current(
        _ appearance: NSAppearance,
        increaseContrast: Bool = NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast,
        reduceTransparency: Bool = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
    ) -> PaneTheme {
        var colors: [String: String] = [:]
        appearance.performAsCurrentDrawingAppearance {
            for (variable, color) in palette { colors[variable] = css(color) }
            for tint in FileTint.allCases { colors["--tint-\(tint.rawValue)"] = css(tint.color) }
        }
        return PaneTheme(
            colors: colors,
            isDark: appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua,
            increaseContrast: increaseContrast, reduceTransparency: reduceTransparency)
    }

    private static func css(_ color: NSColor) -> String {
        guard let rgb = color.usingColorSpace(.sRGB) else { return "rgb(0 0 0)" }
        let channel = { (value: CGFloat) in Int((value * 255).rounded()) }
        let alpha = (rgb.alphaComponent * 1000).rounded() / 1000
        return
            "rgb(\(channel(rgb.redComponent)) \(channel(rgb.greenComponent)) \(channel(rgb.blueComponent)) / \(alpha))"
    }
}
