import SwiftUI

extension Color {
    static let claude = Color(
        nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
                ? NSColor(red: 0xE8 / 255, green: 0x91 / 255, blue: 0x6C / 255, alpha: 1)
                : NSColor(red: 0xC1 / 255, green: 0x5F / 255, blue: 0x3C / 255, alpha: 1)
        })
}

extension Severity {
    var color: Color {
        switch self {
        case .mustFix: .red
        case .suggestion: .orange
        case .nit: .secondary
        case .question: .blue
        }
    }
}

struct ReviewControl: View {
    @Bindable var model: DiffPaneModel
    @State private var showsPopover = false

    var body: some View {
        HStack(spacing: 6) {
            switch model.review {
            case .idle:
                Button {
                    showsPopover = true
                } label: {
                    Label("Ask Claude", systemImage: "sparkles")
                }
                .accessibilityIdentifier("ask-claude")
            case .running(let since):
                HStack(spacing: 6) {
                    ProgressView().controlSize(.small)
                    Text("Reviewing")
                    TimelineView(.periodic(from: since, by: 1)) { context in
                        Text(Self.elapsed(context.date.timeIntervalSince(since))).monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("review-status")
                Button("Stop") { model.stopReview() }
                    .keyboardShortcut(".", modifiers: .command)
                    .accessibilityIdentifier("stop-review")
            case .done(let counts):
                Button {
                    showsPopover = true
                } label: {
                    HStack(spacing: 6) {
                        Text("Review done")
                        ForEach(Severity.allCases.filter { counts[$0, default: 0] > 0 }, id: \.self) { severity in
                            Text("\(counts[severity, default: 0])").foregroundStyle(severity.color)
                                .help(severity.title)
                        }
                    }
                }
                .accessibilityIdentifier("review-status")
            case .failed:
                Button {
                    showsPopover = true
                } label: {
                    Label("Review failed", systemImage: "exclamationmark.triangle")
                }
                .accessibilityIdentifier("review-status")
            }
        }
        .popover(isPresented: $showsPopover, arrowEdge: .bottom) {
            ReviewPopover(model: model) { showsPopover = false }
        }
        .onChange(of: model.review) { _, run in
            if case .failed = run { showsPopover = true }
        }
        .disabled(model.visiblePath == nil && model.review == .idle)
    }

    private static func elapsed(_ seconds: TimeInterval) -> String {
        let total = max(0, Int(seconds))
        return String(format: "%d:%02d", total / 60, total % 60)
    }
}

private struct ReviewPopover: View {
    @Bindable var model: DiffPaneModel
    let dismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(model.reviewTitle).font(.headline)
            if case .failed(let message) = model.review {
                Label(message, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
            FocusChips(selected: $model.reviewFocuses)
            TextField(
                "What should Claude focus on? Optional.", text: $model.reviewInstructions, axis: .vertical
            )
            .lineLimit(3...6)
            .textFieldStyle(.roundedBorder)
            .accessibilityIdentifier("review-instructions")
            HStack {
                Spacer()
                Button("Cancel", action: dismiss).keyboardShortcut(.cancelAction)
                Button(startTitle) {
                    dismiss()
                    model.startReview()
                }
                .keyboardShortcut(.return, modifiers: .command)
                .buttonStyle(.borderedProminent)
                .tint(.claude)
                .accessibilityIdentifier("start-review")
            }
        }
        .padding(16)
        .frame(width: 360)
    }

    private var startTitle: String {
        switch model.review {
        case .failed: "Try Again"
        case .done: "Review Again"
        default: "Start Review"
        }
    }
}

private struct FocusChips: View {
    @Binding var selected: Set<String>

    var body: some View {
        FlowLayout(spacing: 6) {
            ForEach(ReviewFocus.presets, id: \.self) { name in
                let isOn = selected.contains(name)
                Button {
                    if isOn { selected.remove(name) } else { selected.insert(name) }
                } label: {
                    Text(name)
                        .font(.callout)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 3)
                        .foregroundStyle(isOn ? Color.white : Color.primary)
                        .background(isOn ? Color.claude : Color.secondary.opacity(0.15), in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(isOn ? .isSelected : [])
                .accessibilityIdentifier("focus-\(name.lowercased())")
            }
        }
    }
}

private struct FlowLayout: Layout {
    var spacing: Double

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(proposal.width ?? .infinity, subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (subview, origin) in zip(subviews, arrange(bounds.width, subviews).origins) {
            subview.place(at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y), proposal: .unspecified)
        }
    }

    private func arrange(_ width: Double, _ subviews: Subviews) -> (size: CGSize, origins: [CGPoint]) {
        var origins: [CGPoint] = []
        var x = 0.0
        var y = 0.0
        var rowHeight = 0.0
        var maxX = 0.0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            origins.append(CGPoint(x: x, y: y))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
            maxX = max(maxX, x - spacing)
        }
        return (CGSize(width: maxX, height: y + rowHeight), origins)
    }
}
