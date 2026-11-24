import Testing
import WebKit

@testable import diffpilot

@MainActor
struct WebKitFeaturesTests {
    @Test func thePageRenderingCapFeatureExistsOnThisOS() {
        #expect(WebKitFeatures.disable60FPSCap(WKPreferences()))
    }
}
