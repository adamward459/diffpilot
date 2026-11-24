import SwiftUI
import WebKit

enum WebKitFeatures {
    static let near60FPSKey = "PreferPageRenderingUpdatesNear60FPSEnabled"

    // WebKit has no public switch for the 60 fps cap, so this reads its private feature list.
    // A missing feature or selector leaves the page at 60 fps.
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

final class PaneSchemeHandler: NSObject, WKURLSchemeHandler {
    static let scheme = "diffpilot"
    static let entry = URL(string: "diffpilot://pane/diffpane.html")!

    private static let mimeTypes = [
        "html": "text/html", "js": "text/javascript", "css": "text/css", "json": "application/json",
    ]

    func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
        guard let url = task.request.url, let resources = Bundle.main.resourceURL else {
            return task.didFailWithError(URLError(.badURL))
        }
        let file = resources.appendingPathComponent(url.lastPathComponent)
        guard let data = try? Data(contentsOf: file, options: .mappedIfSafe) else {
            return task.didFailWithError(URLError(.fileDoesNotExist))
        }
        let headers = [
            "Content-Type": Self.mimeTypes[file.pathExtension] ?? "application/octet-stream",
            "Content-Length": String(data.count),
        ]
        task.didReceive(HTTPURLResponse(url: url, statusCode: 200, httpVersion: nil, headerFields: headers)!)
        task.didReceive(data)
        task.didFinish()
    }

    func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {}
}

private final class WeakMessageHandler: NSObject, WKScriptMessageHandler {
    weak var model: DiffPaneModel?

    init(_ model: DiffPaneModel) { self.model = model }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let parsed = PageMessage(body: message.body) else { return }
        model?.handle(parsed)
    }
}

struct DiffWebView: NSViewRepresentable {
    static let handlerName = "dp"

    let model: DiffPaneModel

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.setURLSchemeHandler(PaneSchemeHandler(), forURLScheme: PaneSchemeHandler.scheme)
        configuration.userContentController.add(WeakMessageHandler(model), name: Self.handlerName)
        WebKitFeatures.disable60FPSCap(configuration.preferences)
        let web = WKWebView(frame: .zero, configuration: configuration)
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
        web.load(URLRequest(url: PaneSchemeHandler.entry))
        return web
    }

    func updateNSView(_ web: WKWebView, context: Context) {}

    static func dismantleNSView(_ web: WKWebView, coordinator: ()) {
        web.configuration.userContentController.removeScriptMessageHandler(forName: handlerName)
    }
}

struct DiffPaneView: View {
    let model: DiffPaneModel

    var body: some View {
        GeometryReader { proxy in
            DiffWebView(model: model)
                .ignoresSafeArea(.container, edges: .top)
                .onChange(of: proxy.safeAreaInsets.top, initial: true) { _, top in model.setInset(top) }
        }
        .accessibilityIdentifier("diff-pane")
    }
}
