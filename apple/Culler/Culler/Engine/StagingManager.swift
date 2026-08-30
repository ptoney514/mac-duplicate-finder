import Foundation

/// The §9.7 safety net: committed rejects MOVE to `~/Culler Staging/<timestamp>/`
/// with a manifest.json mapping original paths. A separate, explicit action
/// sends staged batches to the macOS Trash (`FileManager.trashItem`).
/// Nothing is ever deleted directly.
enum StagingManager {
    struct Move: Codable, Equatable {
        let original: String
        let staged: String
    }

    nonisolated static var stagingRoot: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Culler Staging", isDirectory: true)
    }

    /// Staged filenames stay unique even when basenames collide.
    static func plan(paths: [String], into folder: URL) -> [Move] {
        var used = Set<String>()
        return paths.map { path in
            let base = (path as NSString).lastPathComponent
            var name = base
            var counter = 1
            while used.contains(name) {
                counter += 1
                let ext = (base as NSString).pathExtension
                let stem = (base as NSString).deletingPathExtension
                name = ext.isEmpty ? "\(stem) (\(counter))" : "\(stem) (\(counter)).\(ext)"
            }
            used.insert(name)
            return Move(original: path, staged: folder.appendingPathComponent(name).path)
        }
    }

    /// Moves `paths` into a new timestamped batch under `root` and writes
    /// the manifest. Returns the batch folder.
    @discardableResult
    static func commit(paths: [String], root: URL = stagingRoot, now: Date = .now) throws -> URL {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HHmmss"
        formatter.locale = Locale(identifier: "en_US_POSIX")
        let folder = root.appendingPathComponent(formatter.string(from: now), isDirectory: true)
        let moves = plan(paths: paths, into: folder)

        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for move in moves {
            try FileManager.default.moveItem(atPath: move.original, toPath: move.staged)
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(moves).write(to: folder.appendingPathComponent("manifest.json"))
        return folder
    }

    /// Existing staged batches, oldest first.
    static func stagedBatches(root: URL = stagingRoot) -> [URL] {
        let contents = (try? FileManager.default.contentsOfDirectory(
            at: root,
            includingPropertiesForKeys: nil,
            options: .skipsHiddenFiles
        )) ?? []
        return contents.filter(\.hasDirectoryPath).sorted { $0.path < $1.path }
    }

    /// Sends every staged batch to the macOS Trash. Returns how many.
    @discardableResult
    static func emptyStaging(root: URL = stagingRoot) throws -> Int {
        let batches = stagedBatches(root: root)
        for batch in batches {
            try FileManager.default.trashItem(at: batch, resultingItemURL: nil)
        }
        return batches.count
    }
}
