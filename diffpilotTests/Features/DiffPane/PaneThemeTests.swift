import AppKit
import Testing

@testable import diffpilot

@MainActor
struct PaneThemeTests {
    private func theme(_ name: NSAppearance.Name) -> PaneTheme {
        PaneTheme.current(NSAppearance(named: name)!)
    }

    @Test func lightAndDarkAppearancesGiveDifferentTextColors() {
        let light = theme(.aqua)
        let dark = theme(.darkAqua)
        #expect(!light.isDark && dark.isDark)
        #expect(light.colors["--text"] != dark.colors["--text"])
        #expect(light.colors["--ed"] != dark.colors["--ed"])
    }

    @Test func accessibilityFlagsPassThroughAndDoNotDependOnTheAppearance() {
        let plain = PaneTheme.current(NSAppearance(named: .aqua)!, increaseContrast: false, reduceTransparency: false)
        #expect(!plain.increaseContrast && !plain.reduceTransparency)
        let strict = PaneTheme.current(
            NSAppearance(named: .darkAqua)!, increaseContrast: true, reduceTransparency: true)
        #expect(strict.increaseContrast && strict.reduceTransparency && strict.isDark)
    }

    @Test func everyColorIsACSSRGBValue() throws {
        let colors = theme(.aqua).colors
        #expect(colors["--accent"] != nil)
        for tint in FileTint.allCases { #expect(colors["--tint-\(tint.rawValue)"] != nil) }
        let pattern = try Regex(#"rgb\(\d{1,3} \d{1,3} \d{1,3} / [0-9.]+\)"#)
        for (name, value) in colors { #expect(value.wholeMatch(of: pattern) != nil, "\(name) = \(value)") }
    }

    @Test func accentColorFollowsTheSystemAccent() throws {
        let accent = try #require(NSColor.controlAccentColor.usingColorSpace(.sRGB))
        let expected = "rgb(\(Int((accent.redComponent * 255).rounded())) "
        #expect(theme(.aqua).colors["--accent"]?.hasPrefix(expected) == true)
    }

    @Test func everyPaneSymbolAndFileIconRendersToAPNG() throws {
        let names = PaneSymbol.allCases.map(\.rawValue) + FileIcon.all.map(\.symbol)
        for name in names {
            let data = try #require(SymbolImages.png(named: name), "\(name) does not exist on this OS")
            #expect(data.prefix(4) == Data([0x89, 0x50, 0x4E, 0x47]))
        }
    }

    @Test func anUnknownSymbolRendersNothing() {
        #expect(SymbolImages.png(named: "no.such.symbol.here") == nil)
    }
}
