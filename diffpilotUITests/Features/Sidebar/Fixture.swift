import Foundation

enum Fixture {
    private static let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent()
        .appendingPathComponent(".fixtures")

    static let large = root.appendingPathComponent("large").path
    static let viewedSeed = root.appendingPathComponent("large-viewed.json").path
}
