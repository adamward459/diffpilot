import Testing

@testable import diffpilot

struct TreeOrderTests {
    @Test func putsFoldersFirstAndIgnoresCase() {
        let paths = [
            "src/b.ts", "README.md", "docs/archive.md", "docs/Cache.md", "src/api/x.ts", "Zeta.md", "src/A.ts",
        ]
        let files = paths.map { FileDiff(path: $0, status: .modified) }
        #expect(
            files.inTreeOrder().map(\.path) == [
                "docs/archive.md", "docs/Cache.md", "src/api/x.ts", "src/A.ts", "src/b.ts", "README.md", "Zeta.md",
            ])
    }
}
