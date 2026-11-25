import XCTest

final class ComparisonUITests: XCTestCase {
    var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = launch()
    }

    override func tearDownWithError() throws {
        // The choice is saved in the app's defaults, which the other UI tests share.
        app.terminate()
        app = launch()
        if branchMenu.title != "feature" { pick(branchItem: "feature") }
        if baseMenu.title != "main" { pick(base: "main") }
        XCTAssertTrue(waitForSummary { $0.contains("2,000 files") }, summary)
        app.terminate()
    }

    private func launch() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_REPO"] = Fixture.large
        app.launchArguments += ["-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
        app.launch()
        XCTAssertTrue(app.staticTexts["summary"].waitForExistence(timeout: 10))
        return app
    }

    private var branchMenu: XCUIElement { app.menuButtons["comparison-branch"] }
    private var baseMenu: XCUIElement { app.menuButtons["comparison-base"] }
    private var summary: String { app.staticTexts["summary"].value as? String ?? "" }

    private func pick(base item: String) {
        baseMenu.click()
        app.menuItems[item].firstMatch.click()
    }

    private func pick(branchItem item: String) {
        branchMenu.click()
        app.menuItems[item].firstMatch.click()
    }

    private func waitForSummary(_ test: @escaping (String) -> Bool) -> Bool {
        let summary = app.staticTexts["summary"]
        let predicate = NSPredicate { _, _ in test(summary.value as? String ?? "") }
        return XCTWaiter.wait(for: [expectation(for: predicate, evaluatedWith: nil)], timeout: 10) == .completed
    }

    func testDefaultsToCheckedOutBranchComparedToMain() {
        XCTAssertEqual(branchMenu.title, "feature")
        XCTAssertEqual(baseMenu.title, "main")
        XCTAssertTrue(summary.contains("2,000 files"), summary)
    }

    func testWorkingTreeOnlyShowsFewerFilesAndIsRestoredOnRelaunch() {
        pick(base: "Working Tree Changes Only")
        XCTAssertTrue(waitForSummary { !$0.contains("2,000 files") && $0.contains("files") }, summary)
        let narrowed = summary

        app.terminate()
        app = launch()
        XCTAssertEqual(baseMenu.title, "Working Tree Changes")
        XCTAssertTrue(waitForSummary { $0 == narrowed }, summary)
    }

    func testReviewingAnotherBranchDisablesUncommittedChoices() {
        pick(branchItem: "main")
        XCTAssertEqual(branchMenu.title, "main")
        baseMenu.click()
        XCTAssertFalse(app.menuItems["Working Tree Changes Only"].firstMatch.isEnabled)
        XCTAssertFalse(app.menuItems["Committed Only"].firstMatch.isEnabled)
        app.typeKey(.escape, modifierFlags: [])
    }
}
