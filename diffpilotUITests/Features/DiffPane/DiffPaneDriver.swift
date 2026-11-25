import AppKit
import Vision
import XCTest

struct DiffPaneDriver {
    static let toolbarHeight = 52.0
    static let fileHeaderHeight = 38.0
    static let hunkHeaderHeight = 28.0
    static let lineHeight = 20.0

    let app: XCUIApplication

    init(appearance: String? = nil) {
        app = XCUIApplication()
        app.launchEnvironment["DIFFPILOT_REPO"] = Fixture.large
        if let appearance { app.launchEnvironment["DIFFPILOT_APPEARANCE"] = appearance }
        app.launchArguments += ["-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
    }

    var window: XCUIElement { app.windows.firstMatch }
    var state: XCUIElement { app.otherElements["diff-pane"] }
    var outline: XCUIElement { app.outlines["file-outline"] }
    var filterField: XCUIElement { app.textFields["filter-field"] }

    var stateValue: String { state.label }
    var topFile: String { field("top") }
    var threadCount: Int { Int(field("threads")) ?? 0 }
    var reply: String { field("reply") }
    var viewedCount: Int { Int(field("viewed")) ?? 0 }

    struct ScrollStats {
        var frames: Int
        var p50: Double
        var p95: Double
        var max: Double
    }

    var scrollStats: ScrollStats? {
        let values = field("scroll").split(separator: "/").compactMap { Double($0) }
        guard values.count == 4 else { return nil }
        return ScrollStats(frames: Int(values[0]), p50: values[1], p95: values[2], max: values[3])
    }

    private func field(_ name: String) -> String {
        for part in stateValue.components(separatedBy: "; ") where part.hasPrefix(name + "=") {
            return String(part.dropFirst(name.count + 1))
        }
        return ""
    }

    func waitUntil(timeout: TimeInterval = 15, pollEvery: TimeInterval = 0.05, _ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            Thread.sleep(forTimeInterval: pollEvery)
        }
        return condition()
    }

    var paneFrame: CGRect { state.frame }

    func paneCoordinate(x: Double, y: Double) -> XCUICoordinate {
        let origin = window.coordinate(withNormalizedOffset: .zero)
        let windowFrame = window.frame
        return origin.withOffset(CGVector(dx: x - windowFrame.minX, dy: y - windowFrame.minY))
    }

    func hoverAndClickLineButton(line index: Int) {
        let pane = paneFrame
        let y = pane.minY + Self.fileHeaderHeight + Self.hunkHeaderHeight + Self.lineHeight * (Double(index) + 0.5)
        paneCoordinate(x: pane.minX + 200, y: y).hover()
        paneCoordinate(x: pane.minX + 71, y: y).click()
    }

    var commentButton: XCUIElement { app.descendants(matching: .any)["comment-on-file"].firstMatch }
    var viewedToggle: XCUIElement { app.descendants(matching: .any)["viewed-toggle"].firstMatch }

    func clickFileCommentButton() { commentButton.click() }

    private static func topLeftFrame(_ screen: NSScreen) -> CGRect {
        let primary = NSScreen.screens[0].frame
        return CGRect(
            x: screen.frame.minX, y: primary.maxY - screen.frame.maxY, width: screen.frame.width,
            height: screen.frame.height)
    }

    func move(to screen: NSScreen) {
        let target = Self.topLeftFrame(screen)
        if !target.contains(CGPoint(x: window.frame.midX, y: window.frame.midY)) {
            let grab = window.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0)).withOffset(
                CGVector(dx: 0, dy: 20))
            let drop = window.coordinate(withNormalizedOffset: .zero).withOffset(
                CGVector(dx: target.midX - window.frame.minX, dy: target.midY - window.frame.minY))
            grab.press(forDuration: 0.2, thenDragTo: drop)
        }
    }

    // Window screenshots fail on a secondary display that is asleep, so the e2e tests use the primary one.
    func moveToPrimaryScreen() { move(to: NSScreen.screens[0]) }

    func moveToFastestScreen() -> NSScreen {
        let screen =
            NSScreen.screens.max { $0.maximumFramesPerSecond < $1.maximumFramesPerSecond } ?? NSScreen.screens[0]
        move(to: screen)
        return screen
    }

    func recognizedText() -> String {
        let image = window.screenshot().image
        guard let full = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return "" }
        let scale = Double(full.width) / window.frame.width
        let pane = paneFrame
        let crop = CGRect(
            x: (pane.minX - window.frame.minX) * scale, y: (pane.minY - window.frame.minY) * scale,
            width: pane.width * scale, height: pane.height * scale)
        guard let cgImage = full.cropping(to: crop) else { return "" }
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        try? VNImageRequestHandler(cgImage: cgImage).perform([request])
        return (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: " ")
    }
}
