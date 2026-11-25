import AppKit

enum PaneSymbol: String, CaseIterable {
    case chevronRight = "chevron.right"
    case chevronDown = "chevron.down"
    case bubble = "bubble.left"
    case plus
    case checkmark
    case sparkles
    case chevronUp = "chevron.up"
    case copy = "doc.on.doc"
    case mustFix = "xmark.circle.fill"
    case suggestion = "exclamationmark.triangle.fill"
    case nit = "circle.fill"
    case question = "questionmark.circle.fill"
}

enum SymbolImages {
    static let renderScale = 3.0
    private static let pointSize = 16.0

    // The first symbol render takes about 80 ms. Doing it off the main thread keeps it out of the first paint.
    static func warmUp() {
        Task.detached(priority: .utility) { _ = png(named: PaneSymbol.plus.rawValue) }
    }

    nonisolated static func png(named name: String) -> Data? {
        let configuration = NSImage.SymbolConfiguration(pointSize: pointSize, weight: .regular)
        guard
            let symbol = NSImage(systemSymbolName: name, accessibilityDescription: nil)?
                .withSymbolConfiguration(configuration)
        else { return nil }
        let width = Int((symbol.size.width * renderScale).rounded(.up))
        let height = Int((symbol.size.height * renderScale).rounded(.up))
        guard
            let bitmap = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4,
                hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
            let context = NSGraphicsContext(bitmapImageRep: bitmap)
        else { return nil }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        symbol.draw(in: NSRect(x: 0, y: 0, width: width, height: height))
        NSGraphicsContext.restoreGraphicsState()
        return bitmap.representation(using: .png, properties: [:])
    }
}
