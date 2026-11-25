import Foundation
import Testing

@testable import diffpilot

struct HostCommandTests {
    @Test func loadPassesFilesAsJSONText() throws {
        let file = FileDiff(
            path: "a.ts", status: .added, additions: 1,
            hunks: [Hunk(header: "@@ -0,0 +1 @@", lines: [DiffLine(kind: .added, old: nil, new: 1, text: "x")])])
        let call = HostCommand.load(files: [file][...], done: true).call
        #expect(call.script == "dp.load(JSON.parse(json), done)")
        #expect(call.arguments["done"] as? Bool == true)
        let json = try #require(call.arguments["json"] as? String)
        let files = try #require(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [[String: Any]])
        #expect(files.first?["path"] as? String == "a.ts")
        #expect(files.first?["status"] as? String == "A")
        #expect(files.first?["additions"] as? Int == 1)
    }

    @Test func otherCommandsCallTheMatchingPageFunction() {
        #expect(HostCommand.removeThread(4).call.script == "dp.removeThread(id)")
        #expect(HostCommand.reveal(path: "a").call.arguments["path"] as? String == "a")
        #expect(HostCommand.setInset(52).call.arguments["points"] as? Double == 52)
    }
}
