import XCTest

@testable import diffpilot

final class RecentFolderTests: XCTestCase {
    private var defaults: UserDefaults!
    private var suite: String!
    private var folder: URL!

    override func setUpWithError() throws {
        suite = "RecentFolderTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)
        folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        defaults.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: folder)
    }

    func testRestoreReturnsNilWithNothingStored() {
        XCTAssertNil(RecentFolder.restore(defaults: defaults, environment: [:]))
    }

    func testRestoreReturnsNilForEmptyData() {
        defaults.set(Data(), forKey: RecentFolder.key)
        XCTAssertNil(RecentFolder.restore(defaults: defaults, environment: [:]))
    }

    func testRestoreReturnsNilForBrokenBookmark() {
        defaults.set(Data([1, 2, 3]), forKey: RecentFolder.key)
        XCTAssertNil(RecentFolder.restore(defaults: defaults, environment: [:]))
    }

    func testRememberThenRestoreReturnsSameFolder() throws {
        RecentFolder.remember(folder, defaults: defaults)
        let restored = try XCTUnwrap(RecentFolder.restore(defaults: defaults, environment: [:]))
        XCTAssertEqual(restored.resolvingSymlinksInPath().path, folder.resolvingSymlinksInPath().path)
    }

    func testEnvironmentWinsOverBookmark() {
        RecentFolder.remember(folder, defaults: defaults)
        let restored = RecentFolder.restore(defaults: defaults, environment: ["DIFFPILOT_REPO": "/tmp/from-env"])
        XCTAssertEqual(restored?.path, "/tmp/from-env")
    }

    func testEmptyEnvironmentValueFallsBackToBookmark() throws {
        RecentFolder.remember(folder, defaults: defaults)
        let restored = try XCTUnwrap(RecentFolder.restore(defaults: defaults, environment: ["DIFFPILOT_REPO": ""]))
        XCTAssertEqual(restored.resolvingSymlinksInPath().path, folder.resolvingSymlinksInPath().path)
    }
}
