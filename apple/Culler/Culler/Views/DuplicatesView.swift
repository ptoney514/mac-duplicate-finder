import SwiftData
import SwiftUI

/// Duplicate review (PRD §9.2): exact groups sorted by reclaimable bytes,
/// one thumbnail per group, keeper defaulting to the oldest copy in the
/// shortest path, per-group override, and batch accept-all. Resolutions are
/// stored in SwiftData; the commit flow arrives in milestone 6.
struct DuplicatesView: View {
    let store: EngineStore
    @Environment(\.modelContext) private var context
    @Query private var resolutions: [DupeResolution]

    var body: some View {
        Group {
            if store.dupeGroups.isEmpty {
                ContentUnavailableView(
                    "No Exact Duplicates",
                    systemImage: "checkmark.seal",
                    description: Text("Scan a folder, then review byte-identical copies here.")
                )
            } else {
                List {
                    Section {
                        summaryHeader
                    }
                    ForEach(store.dupeGroups, id: \.hashHex) { group in
                        DupeGroupRow(
                            group: group,
                            keeperPath: keeperPath(for: group),
                            isResolved: resolution(for: group) != nil,
                            onPickKeeper: { resolve(group, keeperPath: $0) }
                        )
                    }
                }
            }
        }
        .navigationSubtitle(subtitle)
    }

    private var subtitle: String {
        let reclaimable = store.dupeGroups.reduce(0) { $0 + $1.reclaimable }
        return "\(store.dupeGroups.count) groups · " +
            "\(Self.bytes(reclaimable)) reclaimable"
    }

    private var summaryHeader: some View {
        HStack {
            let unresolved = store.dupeGroups.filter { resolution(for: $0) == nil }
            Text(unresolved.isEmpty
                ? "All groups resolved"
                : "\(unresolved.count) groups awaiting review")
                .foregroundStyle(.secondary)
            Spacer()
            Button("Accept All Defaults") {
                acceptAllDefaults()
            }
            .disabled(unresolved.isEmpty)
        }
    }

    private func resolution(for group: DupeGroup) -> DupeResolution? {
        resolutions.first { $0.contentHashHex == group.hashHex }
    }

    /// The chosen keeper, or the engine default (first file: oldest mtime,
    /// shortest path).
    private func keeperPath(for group: DupeGroup) -> String {
        resolution(for: group)?.keeperPath ?? group.files.first?.path ?? ""
    }

    private func resolve(_ group: DupeGroup, keeperPath: String) {
        if let existing = resolution(for: group) {
            existing.keeperPath = keeperPath
            existing.resolvedAt = .now
        } else {
            context.insert(
                DupeResolution(contentHashHex: group.hashHex, keeperPath: keeperPath)
            )
        }
    }

    private func acceptAllDefaults() {
        for group in store.dupeGroups where resolution(for: group) == nil {
            guard let keeper = group.files.first?.path else { continue }
            context.insert(
                DupeResolution(contentHashHex: group.hashHex, keeperPath: keeper)
            )
        }
    }

    static func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(
            fromByteCount: Int64(count),
            countStyle: .file
        )
    }
}

struct DupeGroupRow: View {
    let group: DupeGroup
    let keeperPath: String
    let isResolved: Bool
    let onPickKeeper: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                ThumbCell(path: EngineStore.thumbPath(forHashHex: group.hashHex))
                    .frame(width: 56, height: 56)
                VStack(alignment: .leading) {
                    Text("\(group.files.count) copies × \(DuplicatesView.bytes(group.size))")
                        .font(.headline)
                    Text("\(DuplicatesView.bytes(group.reclaimable)) reclaimable")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if isResolved {
                    Label("Resolved", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                        .labelStyle(.titleAndIcon)
                }
            }
            ForEach(group.files, id: \.path) { file in
                HStack(spacing: 6) {
                    Button {
                        onPickKeeper(file.path)
                    } label: {
                        Image(systemName: file.path == keeperPath ? "star.fill" : "star")
                            .foregroundStyle(file.path == keeperPath ? .yellow : .secondary)
                    }
                    .buttonStyle(.plain)
                    .help("Keep this copy")
                    Text(file.path)
                        .font(.callout)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .foregroundStyle(file.path == keeperPath ? .primary : .secondary)
                }
            }
        }
        .padding(.vertical, 4)
    }
}
