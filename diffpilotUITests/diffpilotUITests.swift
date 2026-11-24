import XCTest

final class SidebarUITests: XCTestCase {
    var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_FIXTURE"] = "design"
        app.launch()
        XCTAssertTrue(app.staticTexts["summary"].waitForExistence(timeout: 10))
    }

    private var outline: XCUIElement { app.outlines["file-outline"] }
    private var filterField: XCUIElement { app.textFields["filter-field"] }
    private var detailPath: XCUIElement { app.staticTexts["detail-path"] }

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
        XCTAssertTrue(summary.contains("12 files"), summary)
        XCTAssertTrue(summary.contains("3/12 viewed"), summary)
        for name in ["docs", "src", "tests", "package.json", "pnpm-lock.yaml"] {
            XCTAssertTrue(row(name).exists, name)
        }
        XCTAssertFalse(row("client.ts").exists)
    }

    func testFilterShowsMatchesAndHidesTheRest() {
        filter("uau")
        XCTAssertTrue(row("useArchiveUndo.ts").waitForExistence(timeout: 5))
        XCTAssertTrue(row("useArchiveUndo.test.ts").exists)
        XCTAssertFalse(row("package.json").exists)
        XCTAssertFalse(row("archive.md").exists)
        XCTAssertFalse(row("MessageRow.tsx").exists)
    }

    func testClearingFilterRestoresEarlierExpansion() {
        row("api").click()
        row("api").typeKey(.rightArrow, modifierFlags: [])
        XCTAssertTrue(row("client.ts").waitForExistence(timeout: 5))

        filter("uau")
        XCTAssertTrue(row("useArchiveUndo.ts").waitForExistence(timeout: 5))
        XCTAssertFalse(row("client.ts").exists)

        clearFilter()
        XCTAssertTrue(row("client.ts").waitForExistence(timeout: 5))
        XCTAssertTrue(row("archive.ts").exists)
        XCTAssertFalse(row("MessageRow.tsx").exists)
    }

    func testHideViewedRemovesViewedFiles() {
        XCTAssertTrue(row("archive.md").exists)
        app.buttons["toggle-hide-viewed"].click()
        XCTAssertEqual(app.buttons["toggle-hide-viewed"].value as? String, "on")
        XCTAssertFalse(row("archive.md").waitForExistence(timeout: 2))
        XCTAssertFalse(row("docs").exists)
        XCTAssertTrue(row("package.json").exists)
        let summary = app.staticTexts["summary"].value as? String ?? ""
        XCTAssertTrue(summary.contains("3/12 viewed"), summary)
    }

    func testOnlyUncommittedShowsTwoFiles() {
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
        XCTAssertEqual(outline.cells.count, 12)
        XCTAssertTrue(outline.staticTexts["src/components/inbox"].exists)
    }

    func testClickingFileShowsPathInDetailPane() {
        XCTAssertTrue(app.staticTexts["Select a file"].exists)
        row("archive.md").click()
        XCTAssertTrue(detailPath.waitForExistence(timeout: 5))
        XCTAssertEqual(detailPath.value as? String, "docs/archive.md")
    }

    func testDownArrowMovesSelection() {
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
