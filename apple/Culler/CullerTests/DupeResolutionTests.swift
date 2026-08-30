import SwiftData
import XCTest

@testable import Culler

final class DupeResolutionTests: XCTestCase {
    @MainActor
    func testResolutionsAreUniquePerContentHash() throws {
        let container = try ModelContainer(
            for: DupeResolution.self,
            configurations: ModelConfiguration(isStoredInMemoryOnly: true)
        )
        let context = container.mainContext

        context.insert(DupeResolution(contentHashHex: "abc123", keeperPath: "/a.jpg"))
        try context.save()

        // Same content hash again: upserts rather than duplicating the row.
        context.insert(DupeResolution(contentHashHex: "abc123", keeperPath: "/b.jpg"))
        try context.save()

        let all = try context.fetch(FetchDescriptor<DupeResolution>())
        XCTAssertEqual(all.count, 1)
        XCTAssertEqual(all.first?.keeperPath, "/b.jpg")
    }
}
