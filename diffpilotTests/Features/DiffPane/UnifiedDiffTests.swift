import Foundation
import Testing

@testable import diffpilot

struct UnifiedDiffTests {
    private func parse(_ lines: String...) -> [FileDiff] {
        UnifiedDiff.parse(lines.joined(separator: "\n") + "\n")
    }

    @Test func parsesHunksWithLineNumbers() throws {
        let files = parse(
            "diff --git a/src/a.ts b/src/a.ts",
            "index 111..222 100644",
            "--- a/src/a.ts",
            "+++ b/src/a.ts",
            "@@ -3,4 +3,5 @@ func x()",
            " keep",
            "-gone",
            "+new one",
            "+new two",
            " tail",
            "@@ -20 +21 @@",
            "-single",
            "+changed")
        let file = try #require(files.first)
        #expect(files.count == 1)
        #expect(file.path == "src/a.ts")
        #expect(file.status == .modified)
        #expect(file.additions == 3)
        #expect(file.deletions == 2)
        #expect(file.hunks.map(\.header) == ["@@ -3,4 +3,5 @@ func x()", "@@ -20 +21 @@"])
        #expect(
            file.hunks[0].lines == [
                DiffLine(kind: .context, old: 3, new: 3, text: "keep"),
                DiffLine(kind: .deleted, old: 4, new: nil, text: "gone"),
                DiffLine(kind: .added, old: nil, new: 4, text: "new one"),
                DiffLine(kind: .added, old: nil, new: 5, text: "new two"),
                DiffLine(kind: .context, old: 5, new: 6, text: "tail"),
            ])
        #expect(
            file.hunks[1].lines == [
                DiffLine(kind: .deleted, old: 20, new: nil, text: "single"),
                DiffLine(kind: .added, old: nil, new: 21, text: "changed"),
            ])
    }

    @Test func newFileStartsAtLineOne() throws {
        let files = parse(
            "diff --git a/docs/new.md b/docs/new.md",
            "new file mode 100644",
            "index 0000000..abc",
            "--- /dev/null",
            "+++ b/docs/new.md",
            "@@ -0,0 +1,2 @@",
            "+one",
            "+two")
        let file = try #require(files.first)
        #expect(file.status == .added)
        #expect(file.path == "docs/new.md")
        #expect(file.additions == 2)
        #expect(file.hunks[0].lines.map(\.new) == [1, 2])
    }

    @Test func deletedFileKeepsItsPath() throws {
        let files = parse(
            "diff --git a/old.md b/old.md",
            "deleted file mode 100644",
            "--- a/old.md",
            "+++ /dev/null",
            "@@ -1,2 +0,0 @@",
            "-one",
            "-two")
        let file = try #require(files.first)
        #expect(file.status == .deleted)
        #expect(file.path == "old.md")
        #expect(file.deletions == 2)
    }

    @Test func renameUsesNewPathAndKeepsOldPath() throws {
        let files = parse(
            "diff --git a/src/Old.ts b/src/New.ts",
            "similarity index 80%",
            "rename from src/Old.ts",
            "rename to src/New.ts",
            "index 1..2 100644",
            "--- a/src/Old.ts",
            "+++ b/src/New.ts",
            "@@ -1 +1 @@",
            "-a",
            "+b")
        let file = try #require(files.first)
        #expect(file.status == .renamed)
        #expect(file.path == "src/New.ts")
        #expect(file.oldPath == "src/Old.ts")
        #expect(file.additions == 1)
    }

    @Test func pureRenameHasNoHunks() throws {
        let files = parse(
            "diff --git a/a.txt b/b.txt",
            "similarity index 100%",
            "rename from a.txt",
            "rename to b.txt")
        #expect(files == [FileDiff(path: "b.txt", oldPath: "a.txt", status: .renamed)])
    }

    @Test func binaryFileIsFlaggedWithoutHunks() throws {
        let files = parse(
            "diff --git a/logo.png b/logo.png",
            "new file mode 100644",
            "index 0000000..abc",
            "Binary files /dev/null and b/logo.png differ")
        let file = try #require(files.first)
        #expect(file.isBinary)
        #expect(file.status == .added)
        #expect(file.path == "logo.png")
        #expect(file.hunks.isEmpty)
    }

    @Test func noNewlineMarkerIsNotALine() throws {
        let files = parse(
            "diff --git a/a.txt b/a.txt",
            "--- a/a.txt",
            "+++ b/a.txt",
            "@@ -1 +1 @@",
            "-old",
            "\\ No newline at end of file",
            "+new",
            "\\ No newline at end of file")
        let file = try #require(files.first)
        #expect(file.hunks[0].lines.count == 2)
        #expect(file.additions == 1)
        #expect(file.deletions == 1)
    }

    @Test func pathWithSpacesComesFromTheHeader() throws {
        let files = parse(
            "diff --git a/my docs/read me.md b/my docs/read me.md",
            "new file mode 100644",
            "--- /dev/null",
            "+++ b/my docs/read me.md",
            "@@ -0,0 +1 @@",
            "+x")
        #expect(files.first?.path == "my docs/read me.md")
    }

    @Test func contentLookingLikeHeadersStaysContent() throws {
        let files = parse(
            "diff --git a/a.md b/a.md",
            "--- a/a.md",
            "+++ b/a.md",
            "@@ -1,2 +1,2 @@",
            "-diff --git a/x b/x",
            "+@@ -1 +1 @@",
            " --- a/y")
        #expect(files.count == 1)
        #expect(files[0].hunks.count == 1)
        #expect(files[0].hunks[0].lines.map(\.text) == ["diff --git a/x b/x", "@@ -1 +1 @@", "--- a/y"])
    }

    @Test func parsesSeveralFilesInOrder() {
        let files = parse(
            "diff --git a/a b/a", "--- a/a", "+++ b/a", "@@ -1 +1 @@", "-x", "+y",
            "diff --git a/b b/b", "--- a/b", "+++ b/b", "@@ -1 +1 @@", "-x", "+y")
        #expect(files.map(\.path) == ["a", "b"])
    }

    @Test func emptyInputHasNoFiles() {
        #expect(UnifiedDiff.parse("").isEmpty)
    }

    @Test func linesEncodeAsCompactArrays() throws {
        let line = DiffLine(kind: .deleted, old: 4, new: nil, text: "x \"q\"")
        let json = String(decoding: try JSONEncoder().encode(line), as: UTF8.self)
        #expect(json == #"["d",4,null,"x \"q\""]"#)
    }
}
