import XCTest

final class DiffPaneUITests: XCTestCase {
    private var pane: DiffPaneDriver!

    override func setUpWithError() throws {
        continueAfterFailure = false
        pane = DiffPaneDriver()
        pane.app.launch()
        XCTAssertTrue(pane.state.waitForExistence(timeout: 10))
        pane.moveToPrimaryScreen()
    }

    func testPaneShowsTheFirstFile() {
        XCTAssertTrue(pane.waitUntil { pane.topFile == "docs/archive.md" }, pane.stateValue)
        XCTAssertTrue(pane.waitUntil { pane.recognizedText().contains("archive.md") })
    }

    func testSelectingASidebarFileShowsItsHeader() {
        XCTAssertTrue(pane.waitUntil { pane.topFile == "docs/archive.md" }, pane.stateValue)
        pane.filterField.click()
        pane.filterField.typeText("useArchiveUndo")
        let row = pane.outline.cells["useArchiveUndo.ts"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 5))
        row.click()
        XCTAssertTrue(pane.waitUntil { pane.topFile == "src/hooks/useArchiveUndo.ts" }, pane.stateValue)
        XCTAssertTrue(pane.waitUntil { pane.recognizedText().contains("useArchiveUndo.ts") })
    }

    func testLineCommentWithClaudeMentionStreamsAReply() {
        XCTAssertTrue(pane.waitUntil { pane.topFile == "docs/archive.md" }, pane.stateValue)
        pane.hoverAndClickLineButton(line: 2)
        XCTAssertTrue(pane.waitUntil { pane.threadCount == 1 }, pane.stateValue)

        pane.app.typeText("@claude is this line needed?")
        pane.app.typeKey(.return, modifierFlags: .command)

        var partialReplies = Set<String>()
        let finished = pane.waitUntil(timeout: 20) {
            let reply = pane.reply
            if !reply.isEmpty, !reply.contains("any code yet.") { partialReplies.insert(reply) }
            return reply.contains("did not change any code yet.")
        }
        XCTAssertTrue(finished, pane.stateValue)
        XCTAssertFalse(partialReplies.isEmpty, "the reply arrived in one piece")
        XCTAssertTrue(pane.waitUntil { pane.recognizedText().contains("change any code") })
    }

    func testFileCommentOpensADraftThread() {
        XCTAssertTrue(pane.waitUntil { pane.topFile == "docs/archive.md" }, pane.stateValue)
        pane.clickFileCommentButton()
        XCTAssertTrue(pane.waitUntil { pane.threadCount == 1 }, pane.stateValue)
        XCTAssertTrue(pane.waitUntil { pane.recognizedText().contains("File comment") })
        pane.app.typeText("Please split this file")
        pane.app.typeKey(.return, modifierFlags: .command)
        XCTAssertTrue(pane.waitUntil { pane.recognizedText().contains("Please split this file") })
        XCTAssertEqual(pane.reply, "")
    }

    func testViewedCheckboxInTheFileHeaderMarksAndUnmarksTheTopFile() {
        XCTAssertTrue(pane.waitUntil { pane.topFile == "docs/archive.md" }, pane.stateValue)
        let before = pane.viewedCount
        pane.clickTopFileViewedCheckbox()
        XCTAssertTrue(pane.waitUntil { pane.viewedCount != before }, pane.stateValue)
        pane.clickTopFileViewedCheckbox()
        XCTAssertTrue(pane.waitUntil { pane.viewedCount == before }, pane.stateValue)
    }

    func testToolbarControlsAreNativeAndSitAboveThePane() {
        XCTAssertTrue(pane.waitUntil { pane.topFile == "docs/archive.md" }, pane.stateValue)
        XCTAssertTrue(pane.commentButton.waitForExistence(timeout: 5))
        XCTAssertLessThan(pane.commentButton.frame.maxY, pane.paneFrame.minY + DiffPaneDriver.toolbarHeight)
        XCTAssertTrue(pane.app.descendants(matching: .any)["pane-mode"].firstMatch.exists)
    }
}
