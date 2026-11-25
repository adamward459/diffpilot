import CoreGraphics
import XCTest

struct CoveredWindow: Error, CustomStringConvertible {
    let owner: String
    var description: String {
        "\(owner) covers the app window, so the screenshot would show it. Move it away and rerun."
    }
}

extension XCUIElement {
    // screenshot() copies this area of the screen, so a window floating above the app would leak into the image.
    func uncoveredScreenshot() throws -> XCUIScreenshot {
        let area = frame
        let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
        for info in (windows as? [[String: Any]]) ?? [] {
            let owner = info[kCGWindowOwnerName as String] as? String ?? ""
            if owner == "diffpilot" { break }
            guard (info[kCGWindowAlpha as String] as? Double ?? 0) > 0,
                (info[kCGWindowLayer as String] as? Int ?? 0) < Int(CGWindowLevelForKey(.dockWindow)),
                let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                let rect = CGRect(dictionaryRepresentation: bounds), rect.intersects(area)
            else { continue }
            throw CoveredWindow(owner: owner)
        }
        return screenshot()
    }
}
