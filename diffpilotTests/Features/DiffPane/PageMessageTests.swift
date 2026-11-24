import Testing

@testable import diffpilot

struct PageMessageTests {
    @Test func parsesSimpleSignals() {
        #expect(PageMessage(body: ["type": "ready"]) == .ready)
        #expect(PageMessage(body: ["type": "needMore"]) == .needMore)
        #expect(PageMessage(body: ["type": "firstPaint", "at": 12.5]) == .firstPaint)
    }

    @Test func parsesLineAndFileThreads() {
        #expect(
            PageMessage(body: ["type": "createThread", "path": "a.ts", "side": "old", "line": 4])
                == .createThread(.line(path: "a.ts", side: .old, line: 4)))
        #expect(PageMessage(body: ["type": "createThread", "path": "a.ts"]) == .createThread(.file(path: "a.ts")))
    }

    @Test func parsesThreadIntents() {
        #expect(PageMessage(body: ["type": "post", "thread": 2, "text": "hi"]) == .post(thread: 2, text: "hi"))
        #expect(PageMessage(body: ["type": "askClaude", "thread": 2]) == .askClaude(thread: 2))
        #expect(PageMessage(body: ["type": "remove", "thread": 2]) == .remove(thread: 2))
    }

    @Test(arguments: [
        "not a dictionary" as Any, [String: Any]() as Any, ["type": 4] as Any, ["type": "unknown"] as Any,
        ["type": "createThread"] as Any, ["type": "post", "thread": "2", "text": "x"] as Any,
        ["type": "post", "thread": 2] as Any, ["type": "askClaude"] as Any, ["type": "remove", "thread": 1.5] as Any,
    ])
    func rejectsMalformedBodies(body: Any) {
        #expect(PageMessage(body: body) == nil)
    }

    @Test func aLineWithoutASideFallsBackToTheFile() {
        #expect(
            PageMessage(body: ["type": "createThread", "path": "a.ts", "line": 4]) == .createThread(.file(path: "a.ts"))
        )
    }
}
