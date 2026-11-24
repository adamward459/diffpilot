import Foundation

nonisolated enum Git {
    enum Failure: Error, Equatable {
        case notFound
        case failed(String)
    }

    // /usr/bin/git is an xcrun shim, and xcrun refuses to run inside the App Sandbox.
    static let candidates = [
        "/Applications/Xcode.app/Contents/Developer/usr/bin/git",
        "/Library/Developer/CommandLineTools/usr/bin/git",
        "/opt/homebrew/bin/git",
        "/usr/local/bin/git",
    ]

    static func run(_ repo: URL, _ args: [String]) throws -> String {
        guard let binary = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
            throw Failure.notFound
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = ["-C", repo.path] + args
        let output = Pipe()
        let errors = Pipe()
        process.standardOutput = output
        process.standardError = errors
        try process.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            let message = String(data: errors.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            throw Failure.failed(message)
        }
        return String(decoding: data, as: UTF8.self)
    }
}
