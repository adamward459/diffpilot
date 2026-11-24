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

    private func field(_ name: String) -> String {
        for part in stateValue.components(separatedBy: "; ") where part.hasPrefix(name + "=") {
            return String(part.dropFirst(name.count + 1))
        }
        return ""
    }

    func waitUntil(timeout: TimeInterval = 15, _ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            Thread.sleep(forTimeInterval: 0.05)
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

    func clickFileCommentButton() {
        let pane = paneFrame
        paneCoordinate(x: pane.maxX - 163, y: pane.minY + Self.fileHeaderHeight / 2).click()
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
