import SwiftData
import SwiftUI

/// Commit flow (§9.7): every file marked reject — triage rejects (all live
/// copies of the content) plus non-keeper copies from duplicate resolutions
/// — with size totals. Confirming MOVES them into a timestamped staging
/// batch with a manifest; a separate action trashes staged batches.
struct CommitView: View {
    let store: EngineStore
    @Query private var decisions: [Decision]
    @Query private var resolutions: [DupeResolution]

    @State private var rejectPaths: [(path: String, size: UInt64)] = []
    @State private var confirmingCommit = false
    @State private var confirmingEmpty = false
    @State private var statusMessage: String?
    @State private var batches: [URL] = []

    var body: some View {
        List {
            Section("Marked for removal") {
                if rejectPaths.isEmpty {
                    Text("Nothing is marked reject. Use Triage, the burst resolver, or Duplicates first.")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(rejectPaths, id: \.path) { entry in
                        HStack {
                            Text(entry.path)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Spacer()
                            Text(DuplicatesView.bytes(entry.size))
                                .foregroundStyle(.secondary)
                                .monospacedDigit()
                        }
                    }
                    HStack {
                        Spacer()
                        Button("Move \(rejectPaths.count) Files to Staging…") {
                            confirmingCommit = true
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }
            }
            Section("Staging") {
                if batches.isEmpty {
                    Text("Staging is empty.")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(batches, id: \.path) { batch in
                        Label(batch.lastPathComponent, systemImage: "shippingbox")
                    }
                    HStack {
                        Spacer()
                        Button("Move Staged Batches to Trash…", role: .destructive) {
                            confirmingEmpty = true
                        }
                    }
                }
            }
            if let statusMessage {
                Section {
                    Text(statusMessage)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .navigationSubtitle(subtitle)
        .task { await reload() }
        .confirmationDialog(
            "Move \(rejectPaths.count) files (\(totalLabel)) to \(StagingManager.stagingRoot.path)?",
            isPresented: $confirmingCommit
        ) {
            Button("Move to Staging") { commit() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Files are moved, never deleted. A manifest records original locations.")
        }
        .confirmationDialog(
            "Move \(batches.count) staged batches to the Trash?",
            isPresented: $confirmingEmpty
        ) {
            Button("Move to Trash", role: .destructive) { emptyStaging() }
            Button("Cancel", role: .cancel) {}
        }
    }

    private var totalBytes: UInt64 {
        rejectPaths.reduce(0) { $0 + $1.size }
    }

    private var totalLabel: String {
        DuplicatesView.bytes(totalBytes)
    }

    private var subtitle: String {
        rejectPaths.isEmpty
            ? "nothing pending"
            : "\(rejectPaths.count) files · \(totalLabel) to reclaim"
    }

    /// Rejects = all live copies of triage-rejected hashes, plus non-keeper
    /// copies from resolved duplicate groups (skipping hashes the user
    /// rejected outright, which already cover every copy).
    private func reload() async {
        batches = StagingManager.stagedBatches()
        let rejectedHashes = decisions.filter { $0.state == .reject }.map(\.contentHashHex)
        var seen = Set<String>()
        var collected: [(String, UInt64)] = []

        for group in await store.filesForHashes(rejectedHashes) {
            for file in group.files where seen.insert(file.path).inserted {
                collected.append((file.path, file.size))
            }
        }
        let rejectedSet = Set(rejectedHashes)
        for resolution in resolutions where !rejectedSet.contains(resolution.contentHashHex) {
            guard let group = store.dupeGroups.first(where: {
                $0.hashHex == resolution.contentHashHex
            }) else { continue }
            for file in group.files
            where file.path != resolution.keeperPath && seen.insert(file.path).inserted {
                collected.append((file.path, group.size))
            }
        }
        rejectPaths = collected.map { (path: $0.0, size: $0.1) }.sorted { $0.path < $1.path }
    }

    private func commit() {
        do {
            let batch = try StagingManager.commit(paths: rejectPaths.map(\.path))
            statusMessage =
                "Moved \(rejectPaths.count) files to \(batch.lastPathComponent). " +
                "Rescan your folders to update the library."
            rejectPaths = []
            batches = StagingManager.stagedBatches()
        } catch {
            statusMessage = "Commit failed: \(error.localizedDescription)"
        }
    }

    private func emptyStaging() {
        do {
            let count = try StagingManager.emptyStaging()
            statusMessage = "Moved \(count) staged batches to the Trash."
            batches = StagingManager.stagedBatches()
        } catch {
            statusMessage = "Emptying staging failed: \(error.localizedDescription)"
        }
    }
}
