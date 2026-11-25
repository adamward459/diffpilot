nonisolated enum UnifiedDiff {
    static let args = ["-c", "core.quotepath=false", "diff", "-U3", "-M", "--no-color"]

    static func parse(_ text: String) -> [FileDiff] {
        var parser = Parser()
        for line in text.utf8.split(separator: newline, omittingEmptySubsequences: false) {
            parser.feed(line)
        }
        return parser.finish()
    }

    private static let newline = UInt8(ascii: "\n")

    private struct Parser {
        var files: [FileDiff] = []
        var file: FileDiff?
        var hunk: Hunk?
        var old = 0
        var new = 0

        mutating func feed(_ line: Substring.UTF8View) {
            if line.starts(with: "diff --git ".utf8) {
                finishFile()
                let path = Self.headerPath(line)
                file = FileDiff(path: path, status: .modified)
            } else if hunk == nil {
                feedHeader(line)
            } else if line.starts(with: "@@".utf8) {
                startHunk(line)
            } else {
                feedBody(line)
            }
        }

        mutating func feedHeader(_ line: Substring.UTF8View) {
            guard file != nil else { return }
            if line.starts(with: "new file".utf8) {
                file?.status = .added
            } else if line.starts(with: "deleted file".utf8) {
                file?.status = .deleted
            } else if line.starts(with: "rename from ".utf8) {
                file?.status = .renamed
                file?.oldPath = Self.string(line.dropFirst("rename from ".utf8.count))
            } else if line.starts(with: "rename to ".utf8) {
                file?.path = Self.string(line.dropFirst("rename to ".utf8.count))
            } else if line.starts(with: "Binary files ".utf8) || line.starts(with: "GIT binary patch".utf8) {
                file?.isBinary = true
            } else if line.starts(with: "@@".utf8) {
                startHunk(line)
            }
        }

        mutating func startHunk(_ line: Substring.UTF8View) {
            finishHunk()
            (old, new) = Self.starts(line)
            hunk = Hunk(header: Self.string(line))
        }

        mutating func feedBody(_ line: Substring.UTF8View) {
            guard let first = line.first else { return }
            let body = Self.string(line.dropFirst())
            switch first {
            case UInt8(ascii: "+"):
                hunk?.lines.append(DiffLine(kind: .added, old: nil, new: new, text: body))
                new += 1
                file?.additions += 1
            case UInt8(ascii: "-"):
                hunk?.lines.append(DiffLine(kind: .deleted, old: old, new: nil, text: body))
                old += 1
                file?.deletions += 1
            case UInt8(ascii: " "):
                hunk?.lines.append(DiffLine(kind: .context, old: old, new: new, text: body))
                old += 1
                new += 1
            default:
                break
            }
        }

        mutating func finishHunk() {
            if let hunk { file?.hunks.append(hunk) }
            hunk = nil
        }

        mutating func finishFile() {
            finishHunk()
            if let file { files.append(file) }
            file = nil
        }

        mutating func finish() -> [FileDiff] {
            finishFile()
            return files
        }

        static func string(_ bytes: some Collection<UInt8>) -> String {
            String(decoding: bytes, as: UTF8.self)
        }

        // For `diff --git a/P b/P` the two halves have equal length, which keeps spaces in P unambiguous.
        static func headerPath(_ line: Substring.UTF8View) -> String {
            let rest = Array(line.dropFirst("diff --git a/".utf8.count))
            let half = (rest.count - 3) / 2
            if half > 0, rest.count == half * 2 + 3 { return string(rest.prefix(half)) }
            let separator = Array(" b/".utf8)
            for index in rest.indices.reversed() where rest[index...].starts(with: separator) {
                return string(rest[(index + separator.count)...])
            }
            return string(rest)
        }

        static func starts(_ line: Substring.UTF8View) -> (Int, Int) {
            var numbers: [Int] = []
            var current: Int?
            var afterComma = false
            for byte in line.dropFirst(2) {
                if byte >= UInt8(ascii: "0"), byte <= UInt8(ascii: "9") {
                    if !afterComma { current = (current ?? 0) * 10 + Int(byte - UInt8(ascii: "0")) }
                } else if byte == UInt8(ascii: ",") {
                    if let value = current { numbers.append(value) }
                    current = nil
                    afterComma = true
                } else {
                    if let value = current { numbers.append(value) }
                    current = nil
                    afterComma = false
                    if byte == UInt8(ascii: "@") { break }
                }
            }
            return (numbers.first ?? 0, numbers.count > 1 ? numbers[1] : 0)
        }
    }
}
