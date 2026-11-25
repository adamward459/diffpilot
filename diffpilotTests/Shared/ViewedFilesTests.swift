import Foundation
import Testing

@testable import diffpilot

struct ViewedStoreTests {
    private func makeURL() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("viewed-\(UUID().uuidString)")
            .appendingPathComponent("viewed.json")
    }

    @Test func roundTripsThroughOneFileAndCreatesTheFolder() {
        let url = makeURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let key = ViewedKey(repo: "/r", branch: "main")
        ViewedStore(url: url).save(key, ["b.ts", "a.ts"])
        #expect(ViewedStore(url: url).load(key) == ["a.ts", "b.ts"])
        let raw = try? JSONDecoder().decode([String: [String]].self, from: Data(contentsOf: url))
        #expect(raw == ["/r\nmain": ["a.ts", "b.ts"]])
    }

    @Test func keepsBranchesOfOneRepoSeparate() {
        let url = makeURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let store = ViewedStore(url: url)
        let main = ViewedKey(repo: "/r", branch: "main")
        let feature = ViewedKey(repo: "/r", branch: "feature")
        let other = ViewedKey(repo: "/other", branch: "main")
        store.save(main, ["a.ts"])
        store.save(feature, ["b.ts"])
        store.save(other, ["c.ts"])
        #expect(store.load(main) == ["a.ts"])
        #expect(store.load(feature) == ["b.ts"])
        #expect(store.load(other) == ["c.ts"])
    }

    @Test func savingAnEmptySetRemovesOnlyThatEntry() {
        let url = makeURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let store = ViewedStore(url: url)
        let main = ViewedKey(repo: "/r", branch: "main")
        let feature = ViewedKey(repo: "/r", branch: "feature")
        store.save(main, ["a.ts"])
        store.save(feature, ["b.ts"])
        store.save(main, [])
        let raw = try? JSONDecoder().decode([String: [String]].self, from: Data(contentsOf: url))
        #expect(raw == ["/r\nfeature": ["b.ts"]])
        #expect(store.load(main).isEmpty)
    }

    @Test func aMissingOrBrokenFileLoadsAsEmpty() throws {
        let url = makeURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let key = ViewedKey(repo: "/r", branch: "main")
        #expect(ViewedStore(url: url).load(key).isEmpty)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("not json".utf8).write(to: url)
        #expect(ViewedStore(url: url).load(key).isEmpty)
    }

    @Test func aStoreWithoutAnURLKeepsEverythingInMemory() {
        let store = ViewedStore()
        let key = ViewedKey(repo: "/r", branch: "main")
        store.save(key, ["a.ts"])
        #expect(store.load(key) == ["a.ts"])
        #expect(ViewedStore().load(key).isEmpty)
    }

    @Test func aSeededStoreAnswersEveryKeyUntilItIsWritten() {
        let store = ViewedStore(seed: ["a.ts", "b.ts"])
        let one = ViewedKey(repo: "/r", branch: "main")
        let two = ViewedKey(repo: "/s", branch: "dev")
        #expect(store.load(one) == ["a.ts", "b.ts"])
        #expect(store.load(two) == ["a.ts", "b.ts"])
        store.save(one, [])
        #expect(store.load(one).isEmpty)
        #expect(store.load(two) == ["a.ts", "b.ts"])
    }

    @Test func readsTheSeedFromTheEnvironmentWithoutTouchingDisk() throws {
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("seed-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: file) }
        try JSONEncoder().encode(["a.ts"]).write(to: file)
        let store = ViewedStore.fromEnvironment(["DIFFPILOT_VIEWED_SEED": file.path])
        #expect(store.url == nil)
        #expect(store.load(ViewedKey(repo: "/r", branch: "main")) == ["a.ts"])
    }

    @Test func buildsTheKeyFromTheComparisonBranchThenTheCheckedOutBranch() {
        let repo = URL(fileURLWithPath: "/a/b/../b/repo")
        let refs = Refs(checkedOut: "feature", detectedBase: "main")
        let checkedOut = Comparison.checkedOut(base: "main", includesUncommitted: true)
        #expect(ViewedKey(repo: repo, comparison: checkedOut, refs: refs).branch == "feature")
        #expect(ViewedKey(repo: repo, comparison: .branch("topic", base: "main"), refs: refs).branch == "topic")
        #expect(ViewedKey(repo: repo, comparison: checkedOut, refs: nil).branch == "HEAD")
        #expect(ViewedKey(repo: repo, comparison: checkedOut, refs: refs).repo == "/a/b/repo")
    }
}

@MainActor
struct ViewedFilesTests {
    @Test func togglesAndWritesThroughOnEveryChange() {
        let store = ViewedStore()
        let key = ViewedKey(repo: "/r", branch: "main")
        let viewed = ViewedFiles(key: key, store: store)
        viewed.toggle("a.ts")
        #expect(viewed.contains("a.ts"))
        #expect(store.load(key) == ["a.ts"])
        viewed.toggle("a.ts")
        #expect(!viewed.contains("a.ts"))
        #expect(store.load(key).isEmpty)
    }

    @Test func loadsTheStoredSetForItsKeyOnly() {
        let store = ViewedStore()
        store.save(ViewedKey(repo: "/r", branch: "main"), ["a.ts"])
        #expect(ViewedFiles(key: ViewedKey(repo: "/r", branch: "main"), store: store).paths == ["a.ts"])
        #expect(ViewedFiles(key: ViewedKey(repo: "/r", branch: "other"), store: store).paths.isEmpty)
    }
}
