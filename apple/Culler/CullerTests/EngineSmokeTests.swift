import XCTest

@testable import Culler

/// Proves the XCFramework + generated bindings work end to end from Swift:
/// open a database, scan a folder with a duplicate pair, query results.
final class EngineSmokeTests: XCTestCase {
    /// 1x1 transparent PNG.
    private static let tinyPNG = Data(
        base64Encoded:
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
    )!

    private final class NullListener: ScanProgressListener {
        func onProgress(progress: ScanProgress) {}
    }

    func testOpenScanDupesAndGridRoundTrip() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("culler-smoke-\(UUID().uuidString)")
        let lib = dir.appendingPathComponent("lib")
        try FileManager.default.createDirectory(at: lib, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        try Self.tinyPNG.write(to: lib.appendingPathComponent("one.png"))
        try Self.tinyPNG.write(to: lib.appendingPathComponent("two.png"))

        let engine = try CullerEngine.open(
            dbPath: dir.appendingPathComponent("culler.db").path
        )
        let summary = try engine.scan(root: lib.path, listener: NullListener())

        XCTAssertEqual(summary.found, 2)
        XCTAssertEqual(summary.analyzed, 2)

        let dupes = try engine.dupes()
        XCTAssertEqual(dupes.count, 1, "identical PNGs form one exact group")
        XCTAssertEqual(dupes.first?.files.count, 2)
        XCTAssertEqual(dupes.first?.hashHex.count, 64)

        let items = try engine.gridItems(offset: 0, limit: 10)
        XCTAssertEqual(items.count, 2)
        XCTAssertNotNil(items.first?.thumbPath)
    }
}
