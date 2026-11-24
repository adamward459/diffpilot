import SwiftUI
import WebKit

enum WebKitFeatures {
    static let near60FPSKey = "PreferPageRenderingUpdatesNear60FPSEnabled"

    // WebKit has no public switch for the 60 fps cap, so this reads its private feature list.
    @discardableResult
    static func disable60FPSCap(_ preferences: WKPreferences) -> Bool {
        let list = NSSelectorFromString("_features")
        let set = NSSelectorFromString("_setEnabled:forFeature:")
        guard WKPreferences.responds(to: list), preferences.responds(to: set),
            let features = WKPreferences.perform(list)?.takeUnretainedValue() as? [NSObject],
            let feature = features.first(where: { $0.value(forKey: "key") as? String == near60FPSKey })
        else { return false }
        typealias Setter = @convention(c) (AnyObject, Selector, Bool, AnyObject) -> Void
        unsafeBitCast(preferences.method(for: set), to: Setter.self)(preferences, set, false, feature)
        return true
    }
}

extension WebKitFeatures {
    // WebKit shrinks the page viewport by the toolbar height. The page needs the full height to scroll under the toolbar.
    static func stopInsettingForToolbar(_ web: WKWebView) {
        let set = NSSelectorFromString("_setAutomaticallyAdjustsContentInsets:")
        guard web.responds(to: set) else { return }
        typealias Setter = @convention(c) (AnyObject, Selector, Bool) -> Void
        unsafeBitCast(web.method(for: set), to: Setter.self)(web, set, false)
    }
}

final class PaneSchemeHandler: NSObject, WKURLSchemeHandler {
    static let scheme = "diffpilot"
    static let entry = URL(string: "diffpilot://pane/diffpane.html")!

    private static let mimeTypes = [
        "html": "text/html", "js": "text/javascript", "css": "text/css", "json": "application/json",
        "png": "image/png",
    ]
    private static let symbolPrefix = "/sf/"

    func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
        guard let url = task.request.url, let resources = Bundle.main.resourceURL else {
            return task.didFailWithError(URLError(.badURL))
        }
        let file = resources.appendingPathComponent("Web").appendingPathComponent(url.lastPathComponent)
        let isSymbol = url.path.hasPrefix(Self.symbolPrefix)
        let data =
            isSymbol
            ? SymbolImages.png(named: url.deletingPathExtension().lastPathComponent)
            : try? Data(contentsOf: file, options: .mappedIfSafe)
        guard let data else { return task.didFailWithError(URLError(.fileDoesNotExist)) }
        let headers = [
            "Content-Type": Self.mimeTypes[isSymbol ? "png" : file.pathExtension] ?? "application/octet-stream",
            "Cache-Control": isSymbol ? "max-age=31536000" : "no-cache",
            "Content-Length": String(data.count),
        ]
        task.didReceive(HTTPURLResponse(url: url, statusCode: 200, httpVersion: nil, headerFields: headers)!)
        task.didReceive(data)
        task.didFinish()
    }

    func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {}
}

private final class PageBridge: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
    weak var model: DiffPaneModel?

    init(_ model: DiffPaneModel) { self.model = model }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let parsed = PageMessage(body: message.body) else { return }
        model?.handle(parsed)
    }

    func webViewWebContentProcessDidTerminate(_ web: WKWebView) {
        model?.pageDidReload()
        web.reload()
    }
}

@MainActor
final class ThemeObserver {
    private var appearance: NSKeyValueObservation?
    private var observers: [(center: NotificationCenter, token: any NSObjectProtocol)] = []

    func start(_ web: WKWebView, model: DiffPaneModel) {
        let apply = { [weak web, weak model] in
            guard let web, let model else { return }
            model.setTheme(PaneTheme.current(web.effectiveAppearance))
        }
        apply()
        appearance = web.observe(\.effectiveAppearance) { _, _ in MainActor.assumeIsolated(apply) }
        let sources = [
            (NotificationCenter.default, NSColor.systemColorsDidChangeNotification),
            (NSWorkspace.shared.notificationCenter, NSWorkspace.accessibilityDisplayOptionsDidChangeNotification),
        ]
        observers = sources.map { center, name in
            (
                center,
                center.addObserver(forName: name, object: nil, queue: .main) { _ in MainActor.assumeIsolated(apply) }
            )
        }
    }

    deinit {
        for (center, token) in observers { center.removeObserver(token) }
    }
}

struct DiffWebView: NSViewRepresentable {
    static let handlerName = "dp"

    let model: DiffPaneModel

    func makeCoordinator() -> ThemeObserver { ThemeObserver() }

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.setURLSchemeHandler(PaneSchemeHandler(), forURLScheme: PaneSchemeHandler.scheme)
        let bridge = PageBridge(model)
        configuration.userContentController.add(bridge, name: Self.handlerName)
        WebKitFeatures.disable60FPSCap(configuration.preferences)
        let web = WKWebView(frame: .zero, configuration: configuration)
        web.navigationDelegate = bridge
        WebKitFeatures.stopInsettingForToolbar(web)
        web.setValue(false, forKey: "drawsBackground")
        #if DEBUG
            web.isInspectable = true
        #endif
        model.send = { [weak web] command in
            let (script, arguments) = command.call
            web?.callAsyncJavaScript(script, arguments: arguments, in: nil, in: .page) { result in
                if case .failure(let error) = result { NSLog("diffpane: \(script) failed: \(error)") }
            }
        }
        context.coordinator.start(web, model: model)
        web.load(URLRequest(url: PaneSchemeHandler.entry))
        return web
    }

    func updateNSView(_ web: WKWebView, context: Context) {}

    static func dismantleNSView(_ web: WKWebView, coordinator: ThemeObserver) {
        web.configuration.userContentController.removeScriptMessageHandler(forName: handlerName)
    }
}

struct DiffPaneView: View {
    let model: DiffPaneModel
    @State private var isPastFirstFrame = false

    var body: some View {
        GeometryReader { proxy in
            if isPastFirstFrame {
                DiffWebView(model: model)
                    .ignoresSafeArea(.container, edges: .top)
            }
            Color.clear.onChange(of: proxy.safeAreaInsets.top, initial: true) { _, top in model.setInset(top) }
        }
        .overlay {
            Color.clear
                .allowsHitTesting(false)
                .accessibilityElement()
                .accessibilityLabel(model.accessibilitySummary)
                .accessibilityIdentifier("diff-pane")
        }
        .task {
            try? await Task.sleep(for: .milliseconds(1))
            isPastFirstFrame = true
        }
    }
}
