import Foundation

nonisolated struct ScriptedReviewer: ClaudeReviewer {
    static let defaultResult = ReviewResult(
        summary: "The archive flow works, but one path skips the undo step.",
        findings: [
            Finding(
                path: "docs/archive.md", line: 2, severity: .mustFix, title: "Undo window is not documented",
                body: "The doc names no undo window. Add the time limit in `docs/archive.md:2`."),
            Finding(
                path: "src/api/archive.ts", line: 9999, severity: .suggestion, title: "Retry is missing",
                body: "A failed archive call is not retried. Wrap it in the shared retry helper."),
            Finding(
                path: "src/api/client.ts", line: nil, severity: .nit, title: "Name is vague",
                body: "Rename `req` to `request`."),
            Finding(
                path: "scripts/missing.py", line: 3, severity: .question, title: "Is this file still used?",
                body: "Nothing in the diff calls this script. Should it stay?"),
        ])

    var result = Self.defaultResult
    var delay = Duration.milliseconds(1500)
    var failure: String?

    struct Failure: Error, LocalizedError {
        var message: String
        var errorDescription: String? { message }
    }

    func review(_ request: ReviewRequest) async throws -> ReviewResult {
        try await Task.sleep(for: delay)
        if let failure { throw Failure(message: failure) }
        return result
    }
}

nonisolated struct ClaudeCLIReviewer: ClaudeReviewer {
    enum Failure: Error, Equatable, LocalizedError {
        case notFound
        case failed(String)

        var errorDescription: String? {
            switch self {
            case .notFound: "The claude command was not found. Install Claude Code, then try again."
            case .failed(let message): message
            }
        }
    }

    static let candidates = [
        "\(NSHomeDirectory())/.local/bin/claude",
        "/opt/homebrew/bin/claude",
        "/usr/local/bin/claude",
    ]

    static let schema = """
        {"type":"object","properties":{"summary":{"type":"string"},"findings":{"type":"array","items":\
        {"type":"object","properties":{"path":{"type":"string"},"line":{"type":["integer","null"]},\
        "severity":{"type":"string","enum":["must-fix","suggestion","nit","question"]},\
        "title":{"type":"string"},"body":{"type":"string"}},"required":["path","severity","title","body"]}}},\
        "required":["summary","findings"]}
        """

    var extraArguments: [String] = []

    static func arguments(for request: ReviewRequest) -> [String] {
        [
            "-p", ReviewPrompt.render(request), "--output-format", "json", "--json-schema", schema,
            "--tools", "Read,Grep,Glob,Bash",
            "--allowedTools", "Bash(git diff:*)", "Bash(git log:*)", "Bash(git show:*)",
        ]
    }

    static func parse(_ data: Data) throws -> ReviewResult {
        struct Envelope: Decodable {
            var isError: Bool?
            var result: String?
            var structuredOutput: ReviewResult?

            enum CodingKeys: String, CodingKey {
                case isError = "is_error"
                case result
                case structuredOutput = "structured_output"
            }
        }
        let envelope: Envelope
        do { envelope = try JSONDecoder().decode(Envelope.self, from: data) } catch {
            throw Failure.failed("Claude returned output that is not valid JSON.")
        }
        if envelope.isError == true { throw Failure.failed(envelope.result ?? "Claude reported an error.") }
        guard let output = envelope.structuredOutput else {
            throw Failure.failed("Claude returned no structured review.")
        }
        return output
    }

    func review(_ request: ReviewRequest) async throws -> ReviewResult {
        guard let binary = Self.candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
            throw Failure.notFound
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = Self.arguments(for: request) + extraArguments
        process.currentDirectoryURL = request.repo
        var environment = ProcessInfo.processInfo.environment
        let home = (binary as NSString).deletingLastPathComponent
        environment["PATH"] = [home, "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].joined(separator: ":")
        process.environment = environment
        process.standardInput = FileHandle.nullDevice
        let run = ProcessRun(process)
        let (status, output, errors) = try await withTaskCancellationHandler {
            try await run.start()
        } onCancel: {
            run.cancel()
        }
        if let result = try? Self.parse(output) { return result }
        guard status == 0 else {
            let message = String(decoding: errors, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            throw Failure.failed(message.isEmpty ? "claude exited with status \(status)." : message)
        }
        return try Self.parse(output)
    }
}

private nonisolated final class ProcessRun: @unchecked Sendable {
    private let process: Process
    private let lock = NSLock()
    private var cancelled = false

    init(_ process: Process) { self.process = process }

    func start() async throws -> (Int32, Data, Data) {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let output = Pipe()
                let errors = Pipe()
                self.process.standardOutput = output
                self.process.standardError = errors
                do {
                    try self.launch()
                } catch {
                    continuation.resume(throwing: error)
                    return
                }
                nonisolated(unsafe) var errorData = Data()
                let group = DispatchGroup()
                group.enter()
                DispatchQueue.global().async {
                    errorData = errors.fileHandleForReading.readDataToEndOfFile()
                    group.leave()
                }
                let data = output.fileHandleForReading.readDataToEndOfFile()
                self.process.waitUntilExit()
                group.wait()
                if self.isCancelled {
                    continuation.resume(throwing: CancellationError())
                } else {
                    continuation.resume(returning: (self.process.terminationStatus, data, errorData))
                }
            }
        }
    }

    func cancel() {
        lock.lock()
        defer { lock.unlock() }
        cancelled = true
        if process.isRunning { process.terminate() }
    }

    private var isCancelled: Bool {
        lock.lock()
        defer { lock.unlock() }
        return cancelled
    }

    private func launch() throws {
        lock.lock()
        defer { lock.unlock() }
        if cancelled { throw CancellationError() }
        try process.run()
    }
}
