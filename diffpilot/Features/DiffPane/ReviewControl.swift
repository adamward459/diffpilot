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

    var symbol: String {
        switch self {
        case .mustFix: "xmark.circle.fill"
        case .suggestion: "exclamationmark.triangle.fill"
        case .nit: "circle.fill"
        case .question: "questionmark.circle.fill"
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
                .popover(isPresented: $showsPopover, arrowEdge: .bottom) {
                    ReviewPopover(model: model) { showsPopover = false }
                }
            Divider().frame(height: 16)
            Button {
                model.commentOnVisibleFile()
            } label: {
                Label("\(model.threads.threads.count)", systemImage: "bubble.left")
                    .labelStyle(.titleAndIcon)
                    .monospacedDigit()
            }
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
                    Image(systemName: "stop.fill").foregroundStyle(.red)
                }
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
                    HStack(spacing: 2) {
                        Image(systemName: severity.symbol).font(.caption)
                        Text("\(counts[severity, default: 0])").fontWeight(.medium).monospacedDigit()
                    }
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
            HStack(spacing: 6, content: label)
        }
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
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "sparkles").foregroundStyle(Color.claude)
                Text("Ask Claude").font(.headline)
                Spacer()
                Text("Claude Code").font(.caption).foregroundStyle(.secondary)
            }
            .padding(.bottom, 12)
            Label(model.reviewTitle, systemImage: "arrow.triangle.branch")
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .padding(.bottom, 14)
            if case .failed(let message) = model.review {
                Label(message, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                    .padding(.bottom, 12)
            }
            Text("What should Claude focus on?").font(.callout.weight(.semibold)).padding(.bottom, 6)
            TextField(
                "Optional. For example “Check error handling in the new API routes” or “This is a perf refactor, look for regressions”.",
                text: $model.reviewInstructions, axis: .vertical
            )
            .lineLimit(2...6)
            .textFieldStyle(.roundedBorder)
            .accessibilityIdentifier("review-instructions")
            .padding(.bottom, 10)
            FocusChips(selected: $model.reviewFocuses).padding(.bottom, 16)
            Text("Scope").font(.callout.weight(.semibold)).padding(.bottom, 6)
            Picker("Scope", selection: $model.reviewScope) {
                Text("All changes (\(model.fileCount) \(model.fileCount == 1 ? "file" : "files"))").tag(ReviewScope.all)
                HStack(spacing: 4) {
                    Text("Only the open file")
                    Text(openFileName).foregroundStyle(.secondary)
                }
                .tag(ReviewScope.openFile)
            }
            .pickerStyle(.radioGroup)
            .labelsHidden()
            .disabled(model.visiblePath == nil)
            .accessibilityIdentifier("review-scope")
            Divider().padding(.vertical, 12)
            HStack {
                ModelMenu(choice: $model.reviewModel)
                Spacer()
                Button("Cancel", action: dismiss)
                    .keyboardShortcut(.cancelAction)
                Button {
                    dismiss()
                    model.startReview()
                } label: {
                    HStack(spacing: 6) {
                        Text(startTitle)
                        Text("⌘↩").font(.caption).opacity(0.75)
                    }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("start-review")
            }
        }
        .padding(16)
        .frame(width: 440)
    }

    private var openFileName: String {
        model.visiblePath.map { ($0 as NSString).lastPathComponent } ?? ""
    }

    private var startTitle: String {
        switch model.review {
        case .failed: "Try Again"
        case .done: "Review Again"
        default: "Start Review"
        }
    }
}

private struct ModelMenu: View {
    @Binding var choice: ReviewModel

    var body: some View {
        Menu {
            Picker("Model", selection: $choice.name) {
                ForEach(ReviewModel.Name.allCases, id: \.self) { Text($0.rawValue.capitalized).tag($0) }
            }
            Picker("Effort", selection: $choice.effort) {
                ForEach(ReviewModel.Effort.allCases, id: \.self) { Text($0.rawValue.capitalized).tag($0) }
            }
        } label: {
            Label(choice.title, systemImage: "sparkles")
        }
        .menuStyle(.button)
        .fixedSize()
        .help("Model for this run")
        .accessibilityIdentifier("review-model")
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
                    Label(name, systemImage: "checkmark")
                        .labelStyle(ChipLabelStyle(isOn: isOn))
                        .font(.callout)
                }
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .tint(isOn ? .accentColor : nil)
                .accessibilityAddTraits(isOn ? .isSelected : [])
                .accessibilityIdentifier("focus-\(name.lowercased())")
            }
        }
    }
}

private struct ChipLabelStyle: LabelStyle {
    let isOn: Bool

    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 4) {
            if isOn { configuration.icon.font(.caption.weight(.semibold)) }
            configuration.title
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
