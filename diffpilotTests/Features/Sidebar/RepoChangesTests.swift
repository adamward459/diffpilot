import Testing

@testable import diffpilot

struct RepoChangesTests {
    @Test func parsesStatusesCountsAndRenames() {
        let changes = RepoChanges.parse(
            nameStatus: "M\0src/a.ts\0A\0src/new.ts\0D\0old.md\0R087\0src/Old.ts\0src/Renamed.ts\0",
            numstat: "3\t1\tsrc/a.ts\0" + "5\t0\tsrc/new.ts\0" + "0\t4\told.md\0"
                + "2\t2\t\0src/Old.ts\0src/Renamed.ts\0",
            uncommitted: "src/new.ts\0")

        #expect(
            changes == [
                FileChange(
                    path: "old.md", status: .deleted, additions: 0, deletions: 4,
                    isUncommitted: false),
                FileChange(
                    path: "src/Renamed.ts", status: .renamed, additions: 2, deletions: 2,
                    isUncommitted: false),
                FileChange(
                    path: "src/a.ts", status: .modified, additions: 3, deletions: 1,
                    isUncommitted: false),
                FileChange(
                    path: "src/new.ts", status: .added, additions: 5, deletions: 0,
                    isUncommitted: true),
            ])
    }

    @Test func emptyDiffHasNoChanges() {
        #expect(RepoChanges.parse(nameStatus: "", numstat: "", uncommitted: "").isEmpty)
    }
}
