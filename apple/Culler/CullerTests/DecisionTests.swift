import SwiftData
import XCTest

@testable import Culler

final class DecisionTests: XCTestCase {
    @MainActor
    func testDecisionsUpsertByContentHash() throws {
        let container = try ModelContainer(
            for: Decision.self,
            configurations: ModelConfiguration(isStoredInMemoryOnly: true)
        )
        let context = container.mainContext

        context.insert(Decision(contentHashHex: "deadbeef", state: .maybe))
        try context.save()
        context.insert(Decision(contentHashHex: "deadbeef", state: .reject))
        try context.save()

        let all = try context.fetch(FetchDescriptor<Decision>())
        XCTAssertEqual(all.count, 1)
        XCTAssertEqual(all.first?.state, .reject)
    }
}
