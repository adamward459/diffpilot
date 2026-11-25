import AppKit

private let rowFont = NSFont.systemFont(ofSize: 12)
private let folderTint = NSColor(srgbRed: 0.863, green: 0.714, blue: 0.478, alpha: 1)

enum Icons {
    static let folder = symbol("folder.fill", size: 14)
    static let bubble = symbol("bubble.left", size: 11)
    static let check = symbol("checkmark", size: 11, weight: .semibold)

    private static var byExtension: [String: (image: NSImage, tint: NSColor)] = [:]

    static func file(named name: String) -> (image: NSImage, tint: NSColor) {
        let ext = name.lastIndex(of: ".").map { String(name[name.index(after: $0)...]).lowercased() } ?? ""
        if let cached = byExtension[ext] { return cached }
        let icon = FileIcon.forFile(named: name)
        let value = (symbol(icon.symbol, size: 13), icon.tint.color)
        byExtension[ext] = value
        return value
    }

    private static func symbol(_ name: String, size: CGFloat, weight: NSFont.Weight = .regular) -> NSImage {
        let image = NSImage(systemSymbolName: name, accessibilityDescription: nil) ?? NSImage()
        return image.withSymbolConfiguration(.init(pointSize: size, weight: weight)) ?? image
    }
}

private func makeLabel(font: NSFont = rowFont) -> NSTextField {
    let label = NSTextField(labelWithString: "")
    label.font = font
    label.lineBreakMode = .byTruncatingTail
    label.maximumNumberOfLines = 1
    label.cell?.usesSingleLineMode = true
    label.translatesAutoresizingMaskIntoConstraints = false
    return label
}

private func makeIcon() -> NSImageView {
    let view = NSImageView()
    view.translatesAutoresizingMaskIntoConstraints = false
    view.setContentHuggingPriority(.required, for: .horizontal)
    view.setContentCompressionResistancePriority(.required, for: .horizontal)
    return view
}

final class FolderCell: NSTableCellView {
    static let id = NSUserInterfaceItemIdentifier("FolderCell")
    private let icon = makeIcon()
    private let label = makeLabel()

    init() {
        super.init(frame: .zero)
        SidebarMetrics.record { $0.cellsCreated += 1 }
        identifier = Self.id
        addSubview(icon)
        addSubview(label)
        imageView = icon
        textField = label
        icon.image = Icons.folder
        icon.contentTintColor = folderTint
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            label.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 6),
            label.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -6),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    func configure(name: String, path: String) {
        SidebarMetrics.record { $0.cellsConfigured += 1 }
        label.stringValue = name
        setAccessibilityLabel(name)
        setAccessibilityIdentifier(path)
    }

    override var backgroundStyle: NSView.BackgroundStyle {
        didSet {
            icon.contentTintColor = backgroundStyle == .emphasized ? .alternateSelectedControlTextColor : folderTint
        }
    }
}

final class BadgeView: NSView {
    private let label = makeLabel(font: .monospacedSystemFont(ofSize: 10.5, weight: .semibold))
    private var fill = NSColor.clear

    init() {
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.cornerRadius = 4
        addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: centerXAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    override var wantsUpdateLayer: Bool { true }

    override var intrinsicContentSize: NSSize {
        NSSize(width: max(16, label.intrinsicContentSize.width + 6), height: 16)
    }

    override func updateLayer() {
        effectiveAppearance.performAsCurrentDrawingAppearance { layer?.backgroundColor = fill.cgColor }
    }

    func set(text: String, fill: NSColor, textColor: NSColor) {
        if label.stringValue != text {
            label.stringValue = text
            invalidateIntrinsicContentSize()
        }
        label.textColor = textColor
        self.fill = fill
        needsDisplay = true
    }
}

final class FileCell: NSTableCellView {
    static let id = NSUserInterfaceItemIdentifier("FileCell")
    private let icon = makeIcon()
    private let name = makeLabel()
    private let parent = makeLabel(font: .systemFont(ofSize: 11))
    private let bubble = makeIcon()
    private let count = makeLabel(font: .systemFont(ofSize: 11))
    private let check = makeIcon()
    private let badge = BadgeView()
    private let commentStack = NSStackView()
    private let trailingStack = NSStackView()

    private var tint = NSColor.secondaryLabelColor
    private var isViewed = false
    private var isUncommitted = false
    private var badgeText = ""

    init() {
        super.init(frame: .zero)
        SidebarMetrics.record { $0.cellsCreated += 1 }
        identifier = Self.id
        imageView = icon
        textField = name
        bubble.image = Icons.bubble
        check.image = Icons.check

        commentStack.setViews([bubble, count], in: .leading)
        commentStack.spacing = 2
        commentStack.translatesAutoresizingMaskIntoConstraints = false
        trailingStack.setViews([commentStack, check, badge], in: .leading)
        trailingStack.spacing = 6
        trailingStack.translatesAutoresizingMaskIntoConstraints = false
        trailingStack.setContentHuggingPriority(.required, for: .horizontal)
        trailingStack.setContentCompressionResistancePriority(.required, for: .horizontal)

        parent.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        name.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)

        addSubview(icon)
        addSubview(name)
        addSubview(parent)
        addSubview(trailingStack)
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            name.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 6),
            name.centerYAnchor.constraint(equalTo: centerYAnchor),
            parent.leadingAnchor.constraint(equalTo: name.trailingAnchor, constant: 6),
            parent.centerYAnchor.constraint(equalTo: centerYAnchor),
            parent.trailingAnchor.constraint(lessThanOrEqualTo: trailingStack.leadingAnchor, constant: -6),
            name.trailingAnchor.constraint(lessThanOrEqualTo: trailingStack.leadingAnchor, constant: -6),
            trailingStack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            trailingStack.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    func configure(node: FileNode, change: FileChange, showsParent: Bool) {
        SidebarMetrics.record { $0.cellsConfigured += 1 }
        let icon = Icons.file(named: node.name)
        self.icon.image = icon.image
        tint = icon.tint
        name.stringValue = node.name
        setAccessibilityLabel(node.name)
        setAccessibilityIdentifier(node.path)
        parent.stringValue = showsParent ? node.parentPath : ""
        commentStack.isHidden = change.commentCount == 0
        if change.commentCount > 0 { count.stringValue = String(change.commentCount) }
        check.isHidden = !change.isViewed
        isViewed = change.isViewed
        isUncommitted = change.isUncommitted
        badgeText = change.status.rawValue
        applyColors()
    }

    override var backgroundStyle: NSView.BackgroundStyle {
        didSet { if oldValue != backgroundStyle { applyColors() } }
    }

    private func applyColors() {
        let selected = backgroundStyle == .emphasized
        let onAccent = NSColor.alternateSelectedControlTextColor
        let secondary = selected ? onAccent.withAlphaComponent(0.85) : .secondaryLabelColor
        name.textColor = selected ? onAccent : isViewed ? .secondaryLabelColor : .labelColor
        parent.textColor = selected ? onAccent.withAlphaComponent(0.7) : .tertiaryLabelColor
        icon.contentTintColor = selected ? onAccent : tint
        bubble.contentTintColor = secondary
        count.textColor = secondary
        check.contentTintColor = secondary
        if selected {
            badge.set(text: badgeText, fill: onAccent.withAlphaComponent(0.22), textColor: secondary)
        } else if isUncommitted {
            badge.set(text: badgeText, fill: NSColor.systemOrange.withAlphaComponent(0.16), textColor: .systemOrange)
        } else {
            badge.set(
                text: badgeText, fill: NSColor.labelColor.withAlphaComponent(0.08), textColor: .secondaryLabelColor)
        }
    }
}
