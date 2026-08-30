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
    /// Burst + near clusters for the resolver, largest bursts first.
    private(set) var clusterDetails: [ClusterDetail] = []
    /// True once the CLIP models are loaded; semantic search available.
    private(set) var modelsReady = false
    /// Non-nil while a search is active; nil shows the whole library.
    private(set) var searchResults: [SearchResult]?
    private(set) var isSearching = false
    var errorMessage: String?

    /// Same location the CLI defaults to, so both tools share one library.
    nonisolated static var supportDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Culler", isDirectory: true)
    }

    nonisolated static var databaseURL: URL {
        supportDirectory.appendingPathComponent("culler.db")
    }

    /// Installed by scripts/fetch-models.sh.
    nonisolated static var modelsDirectory: URL {
        supportDirectory.appendingPathComponent("models", isDirectory: true)
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
        attachModelsIfInstalled()
    }

    /// Loads the CLIP models off the main actor when they're installed.
    private func attachModelsIfInstalled() {
        guard let engine else { return }
        let dir = Self.modelsDirectory
        let marker = dir.appendingPathComponent("vision_model.onnx").path
        guard FileManager.default.fileExists(atPath: marker) else { return }
        Task {
            do {
                try await run { try engine.attachModels(modelsDir: dir.path) }
                modelsReady = true
            } catch {
                errorMessage = "Could not load search models: \(error.localizedDescription)"
            }
        }
    }

    func search(_ query: String) async {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        guard let engine, modelsReady, !trimmed.isEmpty else { return }
        isSearching = true
        defer { isSearching = false }
        do {
            searchResults = try await run { try engine.search(query: trimmed, limit: 60) }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func clearSearch() {
        searchResults = nil
    }

    /// Vision face pass over images that lack face facts, then a cluster
    /// rebuild so keepers reflect eyes-open shots. Safe to re-run; no-op
    /// when everything is already covered.
    func runFacePass() async {
        guard let engine else { return }
        do {
            let targets = try await run { try engine.imagesNeedingFaces() }
            guard !targets.isEmpty else { return }
            let facts: [FaceFacts] = await Task.detached(priority: .utility) {
                targets.compactMap { target in
                    FaceScanner.analyze(thumbPath: target.thumbPath).map {
                        FaceFacts(
                            fileId: target.fileId,
                            faceCount: UInt32($0.faceCount),
                            eyesOpenRatio: $0.eyesOpenRatio
                        )
                    }
                }
            }.value
            if !facts.isEmpty {
                try await run { try engine.storeFaceFacts(facts: facts) }
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    /// Rebuilds near + burst clusters with default thresholds and reloads
    /// the resolver's list (bursts first, then near, big clusters first).
    func refreshClusters() async {
        guard let engine else { return }
        do {
            _ = try await run { try engine.clusterNear(dhashMax: 8, phashMax: 10) }
            _ = try await run { try engine.clusterBursts(maxGapSecs: 3, minCosine: 0.92) }
            let all = try await run { try engine.clusters(kind: nil) }
            clusterDetails = all.sorted {
                ($0.kind == "burst" ? 0 : 1, -$0.members.count, $0.id)
                    < ($1.kind == "burst" ? 0 : 1, -$1.members.count, $1.id)
            }
        } catch {
            errorMessage = error.localizedDescription
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
            await runFacePass()
            await refreshClusters()
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
        case .embedding(let done, let total):
            handler("Embedding… \(done) of \(total)")
        }
    }
}
