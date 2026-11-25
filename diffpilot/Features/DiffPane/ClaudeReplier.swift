nonisolated protocol ClaudeReplier: Sendable {
    func reply(to thread: ReviewThread) -> AsyncStream<String>
}

nonisolated struct ScriptedReplier: ClaudeReplier {
    static let defaultText =
        "I read the change around this line. The `onDismiss` callback now runs after the sheet animates out, so a caller that reads layout inside it sees the final frame. I would keep it, and add a test that dismisses twice in a row. I did not change any code yet."

    var text = Self.defaultText
    var interval = Duration.milliseconds(33)

    func reply(to thread: ReviewThread) -> AsyncStream<String> {
        let words = text.split(separator: " ", omittingEmptySubsequences: false).map { $0 + " " }
        let interval = interval
        return AsyncStream { continuation in
            let task = Task {
                for word in words {
                    if Task.isCancelled { break }
                    continuation.yield(String(word))
                    try? await Task.sleep(for: interval)
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }
}
