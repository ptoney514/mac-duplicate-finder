import SwiftData
import SwiftUI

/// Triage mode (§9.6): one image at a time, keyboard only. J/K or arrows
/// move, 1 keep, 2 maybe, 3 reject (each advances), Z undoes.
struct TriageView: View {
    let store: EngineStore
    @Environment(\.modelContext) private var context
    @Query private var decisions: [Decision]

    @State private var index = 0
    @State private var fullImage: NSImage?
    /// (hash, state before the change) for Z.
    @State private var undoStack: [(String, Decision.State?)] = []

    var body: some View {
        Group {
            if store.libraryItems.isEmpty {
                ContentUnavailableView(
                    "Nothing to Triage",
                    systemImage: "square.grid.3x3.square",
                    description: Text("Scan a folder first.")
                )
            } else {
                triage
            }
        }
        .navigationSubtitle(subtitle)
    }

    private var current: LibraryItem {
        store.libraryItems[min(index, store.libraryItems.count - 1)]
    }

    private var subtitle: String {
        let decided = store.libraryItems.filter { decision(for: $0.contentHashHex) != nil }.count
        return "\(index + 1) of \(store.libraryItems.count) · \(decided) decided"
    }

    private var triage: some View {
        VStack(spacing: 8) {
            ZStack {
                Rectangle().fill(.black.opacity(0.9))
                if let fullImage {
                    Image(nsImage: fullImage)
                        .resizable()
                        .scaledToFit()
                } else {
                    ThumbCell(path: current.thumbPath)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            HStack(spacing: 12) {
                Text(current.path.components(separatedBy: "/").last ?? "")
                    .lineLimit(1)
                    .truncationMode(.middle)
                if let state = decision(for: current.contentHashHex) {
                    Label(state.rawValue.capitalized, systemImage: icon(for: state))
                        .foregroundStyle(color(for: state))
                }
                Spacer()
                Text("J/K move · 1 keep · 2 maybe · 3 reject · Z undo")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal)
            .padding(.bottom, 6)
        }
        .focusable()
        .focusEffectDisabled()
        .task(id: current.fileId) { await loadFullImage() }
        .onKeyPress(.leftArrow) { move(-1) }
        .onKeyPress(.rightArrow) { move(1) }
        .onKeyPress(characters: CharacterSet(charactersIn: "jkJK123zZ")) { press in
            switch press.characters.lowercased() {
            case "j": return move(1)
            case "k": return move(-1)
            case "1": decide(.keep)
            case "2": decide(.maybe)
            case "3": decide(.reject)
            case "z": undo()
            default: return .ignored
            }
            return .handled
        }
    }

    private func move(_ delta: Int) -> KeyPress.Result {
        index = min(max(0, index + delta), store.libraryItems.count - 1)
        return .handled
    }

    /// Decisions write to SwiftData immediately (§9.6) and advance.
    private func decide(_ state: Decision.State) {
        let hash = current.contentHashHex
        guard !hash.isEmpty else { return }
        undoStack.append((hash, decision(for: hash)))
        if let existing = decisions.first(where: { $0.contentHashHex == hash }) {
            existing.state = state
            existing.decidedAt = .now
        } else {
            context.insert(Decision(contentHashHex: hash, state: state))
        }
        _ = move(1)
    }

    private func undo() {
        guard let (hash, previous) = undoStack.popLast() else { return }
        if let existing = decisions.first(where: { $0.contentHashHex == hash }) {
            if let previous {
                existing.state = previous
            } else {
                context.delete(existing)
            }
        } else if let previous {
            context.insert(Decision(contentHashHex: hash, state: previous))
        }
        _ = move(-1)
    }

    private func decision(for hash: String) -> Decision.State? {
        decisions.first { $0.contentHashHex == hash }?.state
    }

    private func loadFullImage() async {
        fullImage = nil
        let path = current.path
        let data = await Task.detached(priority: .userInitiated) {
            try? Data(contentsOf: URL(fileURLWithPath: path))
        }.value
        if let data {
            fullImage = NSImage(data: data)
        }
    }

    private func icon(for state: Decision.State) -> String {
        switch state {
        case .keep: "checkmark.circle.fill"
        case .maybe: "questionmark.circle.fill"
        case .reject: "xmark.circle.fill"
        }
    }

    private func color(for state: Decision.State) -> Color {
        switch state {
        case .keep: .green
        case .maybe: .orange
        case .reject: .red
        }
    }
}
