import Foundation
import os

nonisolated struct MetricCounts: Codable, Equatable, Sendable {
    var cellsCreated = 0
    var cellsConfigured = 0
    var reloads = 0
    var nodesVisited = 0
    var matcherByteCompares = 0
}

nonisolated enum SidebarMetrics {
    private static let counts = OSAllocatedUnfairLock(initialState: MetricCounts())

    static var snapshot: MetricCounts { counts.withLock { $0 } }

    static func reset() { counts.withLock { $0 = MetricCounts() } }

    static func record(_ update: @Sendable (inout MetricCounts) -> Void) {
        #if DEBUG
        counts.withLock { update(&$0) }
        #endif
    }
}
