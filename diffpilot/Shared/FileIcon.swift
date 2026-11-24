import AppKit

nonisolated enum FileTint: String, CaseIterable, Sendable {
    case blue, teal, orange, yellow, purple, secondary

    var color: NSColor {
        switch self {
        case .blue: .systemBlue
        case .teal: .systemTeal
        case .orange: .systemOrange
        case .yellow: .systemYellow
        case .purple: .systemPurple
        case .secondary: .secondaryLabelColor
        }
    }
}

nonisolated struct FileIcon: Equatable, Sendable {
    var symbol: String
    var tint: FileTint

    static func forFile(named name: String) -> FileIcon {
        let ext = name.lastIndex(of: ".").map { String(name[name.index(after: $0)...]).lowercased() } ?? ""
        return switch ext {
        case "ts": FileIcon(symbol: "chevron.left.forwardslash.chevron.right", tint: .blue)
        case "tsx": FileIcon(symbol: "atom", tint: .teal)
        case "swift": FileIcon(symbol: "swift", tint: .orange)
        case "md": FileIcon(symbol: "doc.text", tint: .secondary)
        case "json": FileIcon(symbol: "curlybraces", tint: .yellow)
        case "yaml", "yml": FileIcon(symbol: "list.bullet.indent", tint: .purple)
        default: FileIcon(symbol: "doc", tint: .secondary)
        }
    }

    static let all = ["a.ts", "a.tsx", "a.swift", "a.md", "a.json", "a.yml", "a"].map(forFile(named:))
}
