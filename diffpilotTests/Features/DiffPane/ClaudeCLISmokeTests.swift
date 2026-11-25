import Foundation
import Testing

@testable import diffpilot

struct ClaudeCLISmokeTests {
    static let environment = ProcessInfo.processInfo.environment

    @Test(.enabled(if: environment["DIFFPILOT_SMOKE_REPO"] != nil))
    func theRealCLIReviewsARepo() async throws {
        let repo = URL(fileURLWithPath: try #require(Self.environment["DIFFPILOT_SMOKE_REPO"]))
        let revisions = (Self.environment["DIFFPILOT_SMOKE_REVISIONS"] ?? "HEAD~1 HEAD").split(separator: " ").map(
            String.init)
        let reviewer = ClaudeCLIReviewer(extraArguments: ["--model", "haiku"])
        let result = try await reviewer.review(ReviewRequest(repo: repo, revisions: revisions, focus: "everything"))
        print("SMOKE findings=\(result.findings.count) summary=\(result.summary)")
        if let first = result.findings.first {
            print("SMOKE first=\(first)")
        }
        #expect(!result.summary.isEmpty)
    }
}
