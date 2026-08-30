import Foundation
import Observation

/// Bridges the UniFFI engine into SwiftUI. Published state is main-actor;
/// blocking engine calls hop off via `Task.detached` (the engine handle is
/// Sendable and serializes internally on its own lock).
@MainActor @Observable
final class EngineStore {
    private(set) var engine: CullerEngine?
    private(set) var isScanning = false
    private(set) var progressText = ""
    private(set) var lastSummary: ScanSummary?
    private(set) var dupeGroups: [DupeGroup] = []
    private(set) var libraryItems: [LibraryItem] = []
    var errorMessage: String?

    /// Same location the CLI defaults to, so both tools share one library.
    nonisolated static var supportDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Culler", isDirectory: true)
    }

    nonisolated static var databaseURL: URL {
        supportDirectory.appendingPathComponent("culler.db")
    }

    /// Thumbnails are cached by the engine keyed by content hash (PRD §5.1),
    /// so a dupe group's thumbnail is derivable from its hash alone.
    nonisolated static func thumbPath(forHashHex hex: String) -> String {
        supportDirectory
            .appendingPathComponent("thumbs/\(hex.prefix(2))/\(hex).jpg")
            .path
    }

    func open() {
        guard engine == nil else { return }
        do {
            engine = try CullerEngine.open(dbPath: Self.databaseURL.path)
        } catch {
            errorMessage = "Could not open library database: \(error.localizedDescription)"
        }
    }

    func refresh() async {
        guard let engine else { return }
        do {
            dupeGroups = try await run { try engine.dupes() }
            libraryItems = try await run { try engine.gridItems(offset: 0, limit: 2000) }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func scan(folder: URL) async {
        guard let engine, !isScanning else { return }
        isScanning = true
        progressText = "Starting…"
        defer {
            isScanning = false
            progressText = ""
        }

        let relay = ProgressRelay { [weak self] text in
            Task { @MainActor in self?.progressText = text }
        }
        let root = folder.path
        do {
            lastSummary = try await run { try engine.scan(root: root, listener: relay) }
            await refresh()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func run<T: Sendable>(
        _ work: @escaping @Sendable () throws -> T
    ) async throws -> T {
        try await Task.detached(priority: .userInitiated) { try work() }.value
    }
}

/// Sendable trampoline: engine progress callbacks arrive on the scan thread.
final class ProgressRelay: ScanProgressListener {
    private let handler: @Sendable (String) -> Void

    init(handler: @escaping @Sendable (String) -> Void) {
        self.handler = handler
    }

    func onProgress(progress: ScanProgress) {
        switch progress {
        case .walking(let found):
            handler("Walking… \(found) images found")
        case .hashing(let done, let total):
            handler("Hashing… \(done) of \(total)")
        case .analyzing(let done, let total):
            handler("Analyzing… \(done) of \(total)")
        }
    }
}
