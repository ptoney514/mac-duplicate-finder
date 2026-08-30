import SwiftData
import SwiftUI

/// Burst / near-duplicate resolver (PRD §9.3): filmstrip per cluster with
/// the proposed keeper starred. Keyboard: ←/→ move, Enter accept keeper and
/// advance, K set keeper, R reject all but keeper, M mark all maybe, Space
/// compare the two highest-scored frames.
struct ClustersView: View {
    let store: EngineStore
    @Environment(\.modelContext) private var context
    @Query private var decisions: [Decision]

    @State private var selectedClusterID: Int64?
    @State private var selectedMember = 0
    @State private var keeperOverrides: [Int64: Int64] = [:]
    @State private var showCompare = false

    var body: some View {
        Group {
            if store.clusterDetails.isEmpty {
                ContentUnavailableView(
                    "No Bursts or Similar Shots",
                    systemImage: "square.stack.3d.down.right",
                    description: Text("Scan a folder with capture times and try again.")
                )
            } else {
                HStack(spacing: 0) {
                    clusterList
                        .frame(width: 230)
                    Divider()
                    if let cluster = selectedCluster {
                        resolver(for: cluster)
                    } else {
                        ContentUnavailableView(
                            "Select a Cluster",
                            systemImage: "square.stack",
                            description: Text("Pick a burst or similar group to review.")
                        )
                    }
                }
            }
        }
        .navigationSubtitle("\(store.clusterDetails.count) clusters")
        .task {
            if selectedClusterID == nil {
                selectedClusterID = store.clusterDetails.first?.id
            }
        }
    }

    private var selectedCluster: ClusterDetail? {
        store.clusterDetails.first { $0.id == selectedClusterID }
    }

    private var clusterList: some View {
        List(selection: $selectedClusterID) {
            ForEach(store.clusterDetails, id: \.id) { cluster in
                HStack {
                    ThumbCell(path: cluster.members.first?.thumbPath)
                        .frame(width: 36, height: 36)
                    VStack(alignment: .leading) {
                        Text(cluster.kind == "burst" ? "Burst" : "Similar")
                            .font(.callout)
                        Text("\(cluster.members.count) shots")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    if isResolved(cluster) {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundStyle(.green)
                    }
                }
                .tag(cluster.id)
            }
        }
        .listStyle(.sidebar)
        .onChange(of: selectedClusterID) {
            selectedMember = 0
        }
    }

    private func resolver(for cluster: ClusterDetail) -> some View {
        let members = cluster.members
        let keeperID = keeper(of: cluster)
        let current = members[min(selectedMember, members.count - 1)]

        return VStack(spacing: 10) {
            ZStack {
                ThumbCell(path: current.thumbPath)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            HStack {
                Text(current.path.components(separatedBy: "/").last ?? current.path)
                    .font(.callout)
                    .truncationMode(.middle)
                    .lineLimit(1)
                if let quality = current.qualityScore {
                    Text(String(format: "quality %.3f", quality))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                if current.fileId == keeperID {
                    Label("Keeper", systemImage: "star.fill")
                        .foregroundStyle(.yellow)
                        .font(.caption)
                }
                if let state = decisionState(for: current) {
                    Text(state.rawValue)
                        .font(.caption)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(badgeColor(state).opacity(0.2), in: Capsule())
                }
            }
            ScrollView(.horizontal) {
                HStack(spacing: 6) {
                    ForEach(Array(members.enumerated()), id: \.element.fileId) { index, member in
                        ThumbCell(path: member.thumbPath)
                            .frame(width: 84, height: 84)
                            .overlay(alignment: .topTrailing) {
                                if member.fileId == keeperID {
                                    Image(systemName: "star.fill")
                                        .foregroundStyle(.yellow)
                                        .shadow(radius: 2)
                                        .padding(3)
                                }
                            }
                            .overlay {
                                RoundedRectangle(cornerRadius: 6)
                                    .stroke(
                                        index == selectedMember ? Color.accentColor : .clear,
                                        lineWidth: 3
                                    )
                            }
                            .onTapGesture { selectedMember = index }
                    }
                }
                .padding(.horizontal)
            }
            .frame(height: 96)
            Text("←/→ move · ⏎ accept keeper · K set keeper · R reject others · M maybe · Space compare")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding()
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.leftArrow) {
            selectedMember = max(0, selectedMember - 1)
            return .handled
        }
        .onKeyPress(.rightArrow) {
            selectedMember = min(members.count - 1, selectedMember + 1)
            return .handled
        }
        .onKeyPress(.return) {
            resolve(cluster, keeping: keeperID)
            advance()
            return .handled
        }
        .onKeyPress(.space) {
            showCompare = true
            return .handled
        }
        .onKeyPress(characters: CharacterSet(charactersIn: "krmKRM")) { press in
            switch press.characters.lowercased() {
            case "k":
                keeperOverrides[cluster.id] = current.fileId
            case "r":
                resolve(cluster, keeping: keeper(of: cluster))
            case "m":
                for member in members {
                    upsertDecision(member.contentHashHex, .maybe)
                }
            default:
                return .ignored
            }
            return .handled
        }
        .sheet(isPresented: $showCompare) {
            CompareSheet(members: topTwo(of: cluster))
        }
    }

    /// The engine's proposal unless the user overrode it with K.
    private func keeper(of cluster: ClusterDetail) -> Int64? {
        keeperOverrides[cluster.id] ?? cluster.keeperFileId
    }

    private func topTwo(of cluster: ClusterDetail) -> [ClusterMember] {
        Array(
            cluster.members
                .sorted { ($0.qualityScore ?? 0) > ($1.qualityScore ?? 0) }
                .prefix(2)
        )
    }

    private func resolve(_ cluster: ClusterDetail, keeping keeperID: Int64?) {
        for member in cluster.members {
            upsertDecision(
                member.contentHashHex,
                member.fileId == keeperID ? .keep : .reject
            )
        }
    }

    private func advance() {
        guard let index = store.clusterDetails.firstIndex(where: { $0.id == selectedClusterID })
        else { return }
        let next = store.clusterDetails.index(after: index)
        if next < store.clusterDetails.endIndex {
            selectedClusterID = store.clusterDetails[next].id
        }
    }

    private func isResolved(_ cluster: ClusterDetail) -> Bool {
        cluster.members.allSatisfy { decisionState(for: $0) != nil }
    }

    private func decisionState(for member: ClusterMember) -> Decision.State? {
        decisions.first { $0.contentHashHex == member.contentHashHex }?.state
    }

    private func badgeColor(_ state: Decision.State) -> Color {
        switch state {
        case .keep: .green
        case .reject: .red
        case .maybe: .orange
        }
    }

    private func upsertDecision(_ hashHex: String, _ state: Decision.State) {
        guard !hashHex.isEmpty else { return }
        if let existing = decisions.first(where: { $0.contentHashHex == hashHex }) {
            existing.state = state
            existing.decidedAt = .now
        } else {
            context.insert(Decision(contentHashHex: hashHex, state: state))
        }
    }
}

/// Space-bar comparison of the two highest-scored frames (PRD §9.3).
private struct CompareSheet: View {
    let members: [ClusterMember]
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack {
            HStack(spacing: 12) {
                ForEach(members, id: \.fileId) { member in
                    VStack {
                        ThumbCell(path: member.thumbPath)
                            .frame(width: 380, height: 380)
                        Text(String(format: "quality %.3f", member.qualityScore ?? 0))
                            .font(.caption.monospacedDigit())
                    }
                }
            }
            Button("Close") { dismiss() }
                .keyboardShortcut(.cancelAction)
        }
        .padding()
    }
}
