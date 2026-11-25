import Foundation
import Testing

@testable import diffpilot

@MainActor
struct ThreadStoreTests {
    private let anchor = ThreadAnchor.line(path: "a.ts", side: .new, line: 7)

    @Test func createReusesAThreadOnTheSameAnchor() {
        let store = ThreadStore()
        let first = store.create(anchor)
        let again = store.create(anchor)
        let other = store.create(.file(path: "a.ts"))
        #expect(first.id == again.id)
        #expect(other.id != first.id)
        #expect(store.threads.count == 2)
    }

    @Test func postAddsAUserComment() throws {
        let store = ThreadStore()
        let id = store.create(anchor).id
        let thread = try #require(store.post(id, text: "hi"))
        #expect(thread.comments.map(\.author) == ["You"])
        #expect(thread.comments.map(\.body) == ["hi"])
        #expect(store.post(99, text: "x") == nil)
    }

    @Test func claudeReplyStreamsIntoOneComment() throws {
        let store = ThreadStore()
        let id = store.create(anchor).id
        _ = store.post(id, text: "@claude look")
        let started = try #require(store.beginClaudeReply(id))
        #expect(started.comments.last?.isStreaming == true)
        _ = store.appendClaude(id, "one ", done: false)
        let finished = try #require(store.appendClaude(id, "two", done: true))
        #expect(finished.comments.last?.body == "one two")
        #expect(finished.comments.last?.isStreaming == false)
        #expect(finished.comments.count == 2)
    }

    @Test func aSecondReplyCannotStartWhileOneStreams() {
        let store = ThreadStore()
        let id = store.create(anchor).id
        _ = store.beginClaudeReply(id)
        #expect(store.beginClaudeReply(id) == nil)
    }

    @Test func appendWithoutAStreamingReplyDoesNothing() {
        let store = ThreadStore()
        let id = store.create(anchor).id
        #expect(store.appendClaude(id, "x", done: false) == nil)
    }

    @Test func removeDeletesTheThread() {
        let store = ThreadStore()
        let id = store.create(anchor).id
        store.remove(id)
        #expect(store.threads.isEmpty)
        #expect(store.create(anchor).id != id)
    }

    @Test(arguments: [
        ("@claude check this", true), ("hey @Claude, look", true), ("a\n@claude", true),
        ("mail me@claude.com", false), ("@claudette", false), ("no mention", false),
    ])
    func detectsMentions(text: String, expected: Bool) {
        #expect(ThreadStore.mentionsClaude(text) == expected)
    }

    @Test func threadsRoundTripThroughJSON() throws {
        let thread = ReviewThread(
            id: 3, anchor: anchor, comments: [ThreadComment(id: 1, author: "You", body: "x", isStreaming: false)])
        let data = try JSONEncoder().encode(thread)
        #expect(try JSONDecoder().decode(ReviewThread.self, from: data) == thread)
        let json = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        let anchorJSON = try #require(json["anchor"] as? [String: Any])
        #expect(anchorJSON["kind"] as? String == "line")
        #expect(anchorJSON["side"] as? String == "new")
    }
}
