import Foundation
import Testing

@testable import diffpilot

private func change(_ path: String, uncommitted: Bool = false) -> FileChange {
    FileChange(path: path, status: .modified, additions: 1, deletions: 0, isUncommitted: uncommitted)
}

private func names(_ node: FileNode) -> [String] { node.children.map(\.name) }

private func paths(_ node: FileNode) -> [String] { FileTree.files(in: node).map(\.path) }

private func child(_ node: FileNode, _ name: String) -> FileNode? { node.children.first { $0.name == name } }

struct FileTreeBuildTests {
    @Test func groupsPathsFoldersFirstThenByName() {
        let root = FileTree.build([
            change("src/b.ts"), change("README.md"), change("src/a.ts"), change("docs/x.md"), change("package.json"),
        ])
        #expect(names(root) == ["docs", "src", "package.json", "README.md"])
        #expect(names(child(root, "src")!) == ["a.ts", "b.ts"])
        #expect(child(root, "src")!.isFolder)
        #expect(child(root, "docs")?.path == "docs")
    }

    @Test func mergesSingleChildFolderChains() {
        let root = FileTree.build([
            change("src/components/inbox/Toast.tsx"), change("src/components/inbox/Row.tsx"),
        ])
        #expect(names(root) == ["src/components/inbox"])
        let merged = root.children[0]
        #expect(merged.path == "src/components/inbox")
        #expect(names(merged) == ["Row.tsx", "Toast.tsx"])
    }

    @Test func keepsFolderWhenItAlsoHoldsFilesOrSeveralFolders() {
        let root = FileTree.build([
            change("src/index.ts"), change("src/lib/a.ts"), change("src/api/b.ts"),
        ])
        #expect(names(root) == ["src"])
        #expect(names(child(root, "src")!) == ["api", "lib", "index.ts"])
    }

    @Test func fileNodesExposeNameAndParentPath() throws {
        let root = FileTree.build([change("src/api/archive.ts")])
        let file = try #require(FileTree.files(in: root).first)
        #expect(file.name == "archive.ts")
        #expect(file.parentPath == "src/api")
    }
}

struct FileFilterTests {
    private let sample = FileTree.build([
        change("src/hooks/useArchiveUndo.ts"), change("src/api/client.ts"),
        change("tests/hooks/useArchiveUndo.test.ts"), change("docs/archive.md"), change("package.json"),
    ])

    @Test func keepsMatchingFilesAndTheirAncestorsOnly() {
        let result = FileTree.filter(sample, FileFilter(query: "client"))
        #expect(paths(result) == ["src/api/client.ts"])
        #expect(names(result) == ["src"])
        #expect(names(child(result, "src")!) == ["api"])
    }

    @Test func matchIsCaseInsensitiveOnTheFullPath() {
        let result = FileTree.filter(sample, FileFilter(query: "TESTS/hooks"))
        #expect(paths(result) == ["tests/hooks/useArchiveUndo.test.ts"])
    }

    @Test func emptyQueryKeepsEverything() {
        #expect(paths(FileTree.filter(sample, FileFilter())).count == 5)
    }

    @Test func noMatchLeavesAnEmptyTree() {
        #expect(FileTree.filter(sample, FileFilter(query: "zzzz")).children.isEmpty)
    }

    @Test func fuzzySubsequenceMatches() {
        let result = FileTree.filter(sample, FileFilter(query: "uau"))
        #expect(paths(result) == ["src/hooks/useArchiveUndo.ts", "tests/hooks/useArchiveUndo.test.ts"])
    }

    @Test func fuzzyRequiresCharactersInOrder() {
        #expect(FileTree.filter(sample, FileFilter(query: "udo")).children.isEmpty == false)
        #expect(FileTree.filter(sample, FileFilter(query: "odu")).children.isEmpty)
    }
}

struct FileToggleTests {
    private let sample = FileTree.build([
        change("a/one.ts", uncommitted: true),
        change("a/two.ts", uncommitted: true),
        change("b/three.ts", uncommitted: false),
        change("b/four.ts"),
    ])
    private let viewed: Set<String> = ["a/one.ts", "b/three.ts"]

    @Test func onlyUncommittedDropsCommittedFilesAndEmptyFolders() {
        let result = FileTree.filter(sample, FileFilter(onlyUncommitted: true))
        #expect(paths(result) == ["a/one.ts", "a/two.ts"])
        #expect(names(result) == ["a"])
    }

    @Test func hideViewedDropsViewedFiles() {
        let result = FileTree.filter(sample, FileFilter(hideViewed: true), viewed: viewed)
        #expect(paths(result) == ["a/two.ts", "b/four.ts"])
    }

    @Test func togglesCombineWithQuery() {
        let result = FileTree.filter(
            sample, FileFilter(query: "t", onlyUncommitted: true, hideViewed: true), viewed: viewed)
        #expect(paths(result) == ["a/two.ts"])
    }
}

struct FileListTests {
    @Test func ranksFilenameAndContiguousMatchesFirst() {
        let files = FileTree.files(
            in: FileTree.build([
                change("src/archive/zzz.ts"), change("src/lib/a_r_c_h.ts"), change("docs/archive.md"),
                change("src/api/archive.ts"),
            ]))
        let result = FileTree.list(files, FileFilter(query: "archive")).map(\.path)
        #expect(Set(result.prefix(2)) == ["docs/archive.md", "src/api/archive.ts"])
        #expect(result.last == "src/archive/zzz.ts")
        #expect(result.count == 3)
    }

    @Test func emptyQueryKeepsOrderAndAppliesToggles() {
        let files = FileTree.files(in: FileTree.build([change("a.ts"), change("b.ts")]))
        #expect(FileTree.list(files, FileFilter()).map(\.path) == ["a.ts", "b.ts"])
        #expect(FileTree.list(files, FileFilter(hideViewed: true), viewed: ["a.ts"]).map(\.path) == ["b.ts"])
    }
}

struct SampleChangesTests {
    @Test func isDeterministicAndUnique() {
        let first = SampleChanges.generate(count: 500)
        #expect(first == SampleChanges.generate(count: 500))
        #expect(Set(first.map(\.path)).count == 500)
    }
}

struct FilePerformanceTests {
    @Test func buildAndFilterTenThousandFilesUnder50ms() {
        #if DEBUG
            let budget = Duration.milliseconds(500)
        #else
            let budget = Duration.milliseconds(50)
        #endif
        let changes = SampleChanges.generate(count: 10_000)
        let clock = ContinuousClock()

        var root = FileNode.empty
        let build = clock.measure { root = FileTree.build(changes) }

        var tree = FileNode.empty
        let filterTree = clock.measure { tree = FileTree.filter(root, FileFilter(query: "uau")) }

        var listed: [FileNode] = []
        let all = FileTree.files(in: root)
        let filterList = clock.measure { listed = FileTree.list(all, FileFilter(query: "uau")) }

        print("PERF build=\(build) filterTree=\(filterTree) filterList=\(filterList) matches=\(listed.count)")
        #expect(FileTree.files(in: root).count == 10_000)
        #expect(FileTree.files(in: tree).count == listed.count)
        #expect(build < budget)
        #expect(filterTree < budget)
        #expect(filterList < budget)
    }
}
