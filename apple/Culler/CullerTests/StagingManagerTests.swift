import XCTest

@testable import Culler

final class StagingManagerTests: XCTestCase {
    private func tempDir() -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("culler-staging-\(UUID().uuidString)")
        try! FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    func testPlanKeepsCollidingBasenamesUnique() {
        let folder = URL(fileURLWithPath: "/staging/batch")
        let moves = StagingManager.plan(
            paths: ["/a/IMG_1.jpg", "/b/IMG_1.jpg", "/c/IMG_1.jpg", "/d/other.png"],
            into: folder
        )
        let names = moves.map { ($0.staged as NSString).lastPathComponent }
        XCTAssertEqual(names, ["IMG_1.jpg", "IMG_1 (2).jpg", "IMG_1 (3).jpg", "other.png"])
        XCTAssertEqual(Set(names).count, names.count)
    }

    func testCommitMovesFilesAndWritesManifest() throws {
        let dir = tempDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let lib = dir.appendingPathComponent("lib")
        try FileManager.default.createDirectory(at: lib, withIntermediateDirectories: true)
        let fileA = lib.appendingPathComponent("a.jpg")
        let fileB = lib.appendingPathComponent("b.jpg")
        try Data([1, 2, 3]).write(to: fileA)
        try Data([4, 5]).write(to: fileB)
        let root = dir.appendingPathComponent("staging")

        let batch = try StagingManager.commit(paths: [fileA.path, fileB.path], root: root)

        XCTAssertFalse(FileManager.default.fileExists(atPath: fileA.path), "moved, not copied")
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: batch.appendingPathComponent("a.jpg").path)
        )
        let manifestData = try Data(contentsOf: batch.appendingPathComponent("manifest.json"))
        let moves = try JSONDecoder().decode([StagingManager.Move].self, from: manifestData)
        XCTAssertEqual(moves.count, 2)
        XCTAssertEqual(moves.first?.original, fileA.path)
        // Compare resolved paths: temp dirs mix /var and /private/var.
        XCTAssertEqual(
            StagingManager.stagedBatches(root: root)
                .map { $0.resolvingSymlinksInPath().lastPathComponent },
            [batch.resolvingSymlinksInPath().lastPathComponent]
        )
    }
}
