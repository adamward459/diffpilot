import XCTest

final class SidebarUITests: XCTestCase {
    var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_REPO"] = Fixture.large
        app.launchArguments += ["-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
        app.launch()
        XCTAssertTrue(app.staticTexts["summary"].waitForExistence(timeout: 10))
    }

    private var outline: XCUIElement { app.outlines["file-outline"] }
    private var filterField: XCUIElement { app.textFields["filter-field"] }
    private var pane: XCUIElement { app.otherElements["diff-pane"] }

    private func row(_ name: String) -> XCUIElement { outline.cells[name].firstMatch }

    private func filter(_ text: String) {
        filterField.click()
        filterField.typeText(text)
    }

    private func clearFilter() {
        filterField.click()
        filterField.typeKey("a", modifierFlags: .command)
        filterField.typeKey(.delete, modifierFlags: [])
    }

    private func expandEverything() {
        let collapsed = outline.disclosureTriangles.matching(NSPredicate(format: "value == 0"))
        while collapsed.count > 0 { collapsed.firstMatch.click() }
    }

    func testLaunchShowsSummaryAndTopFolders() {
        let summary = app.staticTexts["summary"].value as? String ?? ""
        XCTAssertTrue(summary.contains("2,000 files"), summary)
        XCTAssertTrue(summary.contains("493/2,000 viewed"), summary)
        for name in ["docs", "packages", "scripts", "src", "tests"] {
            XCTAssertTrue(row(name).exists, name)
        }
        XCTAssertFalse(row("client.ts").exists)
    }

    func testFilterShowsMatchesAndHidesTheRest() {
        filter("useArchiveUndo")
        XCTAssertTrue(row("useArchiveUndo.ts").waitForExistence(timeout: 5))
        XCTAssertTrue(row("useArchiveUndo.test.ts").exists)
        XCTAssertFalse(row("package.json").exists)
        XCTAssertFalse(row("archive.md").exists)
        XCTAssertFalse(row("MessageRow.tsx").exists)
    }

    func testClearingFilterRestoresEarlierExpansion() {
        filter("archive")
        XCTAssertTrue(row("archive.md").waitForExistence(timeout: 5))
        clearFilter()
        row("docs").click()
        row("docs").typeKey(.leftArrow, modifierFlags: [])
        XCTAssertFalse(row("archive.md").waitForExistence(timeout: 2))

        filter("archive")
        XCTAssertTrue(row("archive.md").waitForExistence(timeout: 5))

        clearFilter()
        XCTAssertTrue(row("docs").waitForExistence(timeout: 5))
        XCTAssertFalse(row("archive.md").exists)
        XCTAssertTrue(row("src").exists)
    }

    func testHideViewedRemovesViewedFiles() {
        filter("archive")
        XCTAssertTrue(row("archive.md").waitForExistence(timeout: 5))
        app.buttons["toggle-hide-viewed"].click()
        XCTAssertEqual(app.buttons["toggle-hide-viewed"].value as? String, "on")
        XCTAssertFalse(row("archive.md").waitForExistence(timeout: 2))
        XCTAssertFalse(row("docs").exists)
        XCTAssertTrue(row("archive.ts").exists)
        let summary = app.staticTexts["summary"].value as? String ?? ""
        XCTAssertTrue(summary.contains("493/2,000 viewed"), summary)
    }

    func testOnlyUncommittedShowsTwoArchiveFiles() {
        filter("archive")
        app.buttons["toggle-uncommitted"].click()
        XCTAssertFalse(row("package.json").waitForExistence(timeout: 2))
        expandEverything()
        XCTAssertTrue(row("useArchiveUndo.ts").exists)
        XCTAssertTrue(row("archive.test.ts").exists)
        let files = outline.cells.matching(NSPredicate(format: "label CONTAINS '.'"))
        XCTAssertEqual(files.count, 2)
    }

    func testListModeShowsFilesFlatWithParentPath() {
        app.buttons["mode-button"].click()
        XCTAssertEqual(app.buttons["mode-button"].label, "Switch to tree")
        XCTAssertTrue(row("ArchiveToast.tsx").waitForExistence(timeout: 5))
        XCTAssertFalse(row("docs").exists)
        XCTAssertFalse(row("src").exists)
        filter("archive")
        XCTAssertEqual(outline.cells.count, 6)
        XCTAssertTrue(outline.staticTexts["src/components/inbox"].exists)
    }

    func testClickingFileShowsItInTheDiffPane() {
        XCTAssertTrue(pane.waitForExistence(timeout: 5))
        row("Cache.md").click()
        let shown = NSPredicate(format: "label CONTAINS %@", "top=docs/Cache.md;")
        expectation(for: shown, evaluatedWith: pane)
        waitForExpectations(timeout: 10)
    }

    func testDownArrowMovesSelection() {
        filter("archive")
        XCTAssertTrue(row("archive.md").waitForExistence(timeout: 5))
        row("archive.md").click()
        XCTAssertTrue(row("archive.md").isSelected)
        row("archive.md").typeKey(.downArrow, modifierFlags: [])
        XCTAssertTrue(row("src").isSelected)
        XCTAssertFalse(row("archive.md").isSelected)
    }

    func testLeftArrowCollapsesAndRightArrowExpandsFolder() {
        row("docs").click()
        XCTAssertTrue(row("archive.md").exists)
        row("docs").typeKey(.leftArrow, modifierFlags: [])
        XCTAssertFalse(row("archive.md").waitForExistence(timeout: 2))
        row("docs").typeKey(.rightArrow, modifierFlags: [])
        XCTAssertTrue(row("archive.md").waitForExistence(timeout: 5))
    }
}
