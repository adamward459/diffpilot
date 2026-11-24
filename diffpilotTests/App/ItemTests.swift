import Foundation
import SwiftData
import Testing

@testable import diffpilot

@MainActor
struct ItemTests {
    @Test func savedItemIsFetchedWithItsTimestamp() throws {
        let container = try ModelContainer(
            for: Item.self, configurations: ModelConfiguration(isStoredInMemoryOnly: true))
        let timestamp = Date(timeIntervalSince1970: 1_700_000_000)
        container.mainContext.insert(Item(timestamp: timestamp))

        let items = try container.mainContext.fetch(FetchDescriptor<Item>())
        #expect(items.map(\.timestamp) == [timestamp])
    }
}
