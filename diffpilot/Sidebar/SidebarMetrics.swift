import Foundation
import os

nonisolated struct MetricCounts: Codable, Equatable, Sendable {
    var cellsCreated = 0
    var cellsConfigured = 0
    var reloads = 0
    var nodesVisited = 0
    var matcherByteCompares = 0
}

nonisolated final class MetricCounter: Sendable {
    private let state = OSAllocatedUnfairLock(initialState: MetricCounts())

    var snapshot: MetricCounts { state.withLock { $0 } }

    func update(_ change: @Sendable (inout MetricCounts) -> Void) { state.withLock { change(&$0) } }
}

nonisolated enum SidebarMetrics {
    static let app = MetricCounter()
    @TaskLocal static var scoped: MetricCounter?

    static var snapshot: MetricCounts { app.snapshot }

    static func record(_ change: @Sendable (inout MetricCounts) -> Void) {
        #if DEBUG
            (scoped ?? app).update(change)
        #endif
    }

    static func measure(_ body: () -> Void) -> MetricCounts {
        let counter = MetricCounter()
        $scoped.withValue(counter, operation: body)
        return counter.snapshot
    }
}
