import Foundation
import Observation

@Observable
final class ComparisonModel {
    let repo: URL
    private(set) var refs: Refs?
    private(set) var comparison: Comparison?
    private let defaults: UserDefaults

    init(repo: URL, defaults: UserDefaults = .standard) {
        self.repo = repo
        self.defaults = defaults
    }

    func load() async {
        let repo = repo
        let defaults = defaults
        (refs, comparison) = await Task.detached(priority: .userInitiated) {
            let refs = Refs.load(repo: repo)
            let saved = ComparisonStore.load(repo: repo, checkedOut: refs.checkedOut, defaults: defaults)
            let restored = saved.flatMap { saved in
                saved.refs.allSatisfy { refs.contains($0) || Self.resolves($0, in: repo) } ? saved : nil
            }
            return (refs, restored ?? .checkedOut(base: refs.detectedBase, includesUncommitted: true))
        }.value
    }

    func select(_ next: Comparison) {
        guard let refs, next != comparison else { return }
        comparison = next
        ComparisonStore.save(next, repo: repo, checkedOut: refs.checkedOut, defaults: defaults)
    }

    private nonisolated static func resolves(_ ref: String, in repo: URL) -> Bool {
        (try? Git.run(repo, ["rev-parse", "--verify", "-q", "\(ref)^{commit}"])) != nil
    }
}
