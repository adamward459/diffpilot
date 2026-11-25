import XCTest

final class SidebarWatchUITests: XCTestCase {
    private let repo = URL(fileURLWithPath: Fixture.large)
    private var file: URL { repo.appendingPathComponent("docs/zz-watch-test.md") }

    override func setUpWithError() throws {
        continueAfterFailure = false
        try XCTSkipUnless(
            FileManager.default.isWritableFile(atPath: repo.path), "The test runner cannot write to the fixture repo.")
    }

    override func tearDown() {
        _ = try? git(["rm", "-q", "-f", "--cached", "--ignore-unmatch", "docs/zz-watch-test.md"])
        try? FileManager.default.removeItem(at: file)
    }

    private func git(_ args: [String]) throws -> Int32 {
        let binary = [
            "/Applications/Xcode.app/Contents/Developer/usr/bin/git", "/Library/Developer/CommandLineTools/usr/bin/git",
            "/opt/homebrew/bin/git", "/usr/local/bin/git", "/usr/bin/git",
        ].first { FileManager.default.isExecutableFile(atPath: $0) }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: try XCTUnwrap(binary))
        process.arguments = ["-C", repo.path] + args
        try process.run()
        process.waitUntilExit()
        return process.terminationStatus
    }

    func testAddingAFileToTheRepoUpdatesTheSidebarWithoutAReload() throws {
        let app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_REPO"] = Fixture.large
        app.launchEnvironment["DIFFPILOT_VIEWED_SEED"] = Fixture.viewedSeed
        app.launchArguments += ["-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
        app.launch()
        let summary = app.staticTexts["summary"]
        XCTAssertTrue(summary.waitForExistence(timeout: 10))
        XCTAssertTrue((summary.value as? String ?? "").contains("2,000 files"))

        try "# watched\n".write(to: file, atomically: true, encoding: .utf8)
        XCTAssertEqual(try git(["add", "docs/zz-watch-test.md"]), 0)

        let grown = NSPredicate(format: "value CONTAINS %@", "2,001 files")
        expectation(for: grown, evaluatedWith: summary)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.outlines["file-outline"].cells["zz-watch-test.md"].waitForExistence(timeout: 5))
    }
}
