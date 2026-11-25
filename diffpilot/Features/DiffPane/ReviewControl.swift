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

struct ClaudePill: View {
    @Bindable var model: DiffPaneModel
    @State private var showsPopover = false
    @State private var showsResult = false

    var body: some View {
        HStack(spacing: 0) {
            claude
                .padding(.horizontal, 10)
                .frame(height: 28)
                .popover(isPresented: $showsPopover, arrowEdge: .bottom) {
                    ReviewPopover(model: model) { showsPopover = false }
                }
            Divider().frame(height: 16)
            Button {
                model.commentOnVisibleFile()
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: "bubble.left")
                    let count = model.threads.threads.count
                    if count > 0 { Text("\(count)").monospacedDigit() }
                }
                .padding(.horizontal, 10)
                .frame(height: 28)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Comment on file")
            .disabled(model.visiblePath == nil)
            .accessibilityLabel("Comment on file")
            .accessibilityIdentifier("comment-on-file")
        }
        .onChange(of: model.review) { _, run in
            if case .failed = run { showsPopover = true }
            showsResult = if case .done = run { true } else { false }
        }
        .task(id: showsResult) {
            guard showsResult else { return }
            try? await Task.sleep(for: .seconds(5))
            showsResult = false
        }
    }

    @ViewBuilder private var claude: some View {
        switch model.review {
        case .running(let since):
            HStack(spacing: 7) {
                ProgressView().controlSize(.small)
                HStack(spacing: 4) {
                    Text("Reviewing").fontWeight(.semibold)
                    TimelineView(.periodic(from: since, by: 1)) { context in
                        Text(Self.elapsed(context.date.timeIntervalSince(since))).monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("review-status")
                Button {
                    model.stopReview()
                } label: {
                    Image(systemName: "stop.fill").font(.system(size: 9))
                        .frame(width: 18, height: 18)
                        .background(Color.secondary.opacity(0.2), in: Circle())
                }
                .buttonStyle(.plain)
                .keyboardShortcut(".", modifiers: .command)
                .help("Stop Claude (⌘.)")
                .accessibilityLabel("Stop")
                .accessibilityIdentifier("stop-review")
            }
        case .done(let counts) where showsResult:
            pillButton {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                Text("Review done")
                ForEach(Severity.allCases.filter { counts[$0, default: 0] > 0 }, id: \.self) { severity in
                    Text("\(counts[severity, default: 0])").fontWeight(.semibold).monospacedDigit()
                        .foregroundStyle(severity.color)
                        .help(severity.title)
                }
            }
            .accessibilityIdentifier("review-status")
        case .failed:
            pillButton {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                Text("Review failed")
            }
            .accessibilityIdentifier("review-status")
        default:
            pillButton {
                Image(systemName: "sparkles").foregroundStyle(Color.claude)
                Text("Ask Claude")
            }
            .help("Ask Claude")
            .disabled(model.visiblePath == nil)
            .accessibilityIdentifier("ask-claude")
        }
    }

    private func pillButton(@ViewBuilder _ label: () -> some View) -> some View {
        Button {
            showsPopover = true
        } label: {
            HStack(spacing: 6, content: label).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
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
