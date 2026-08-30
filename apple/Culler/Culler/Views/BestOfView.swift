import SwiftUI

/// Best-of view (§9.5): the highest-quality image per gap-based period.
struct BestOfView: View {
    let store: EngineStore

    enum Period: String, CaseIterable, Identifiable {
        case events = "Events (2h gaps)"
        case days = "Days (24h gaps)"

        var id: String { rawValue }
        var gapSecs: Int64 {
            switch self {
            case .events: 2 * 3600
            case .days: 24 * 3600
            }
        }
    }

    @State private var period: Period = .events

    private let columns = [
        GridItem(.adaptive(minimum: 160, maximum: 220), spacing: 10)
    ]

    var body: some View {
        Group {
            if store.bestOfEntries.isEmpty {
                ContentUnavailableView(
                    "No Dated Photos",
                    systemImage: "trophy",
                    description: Text("Best-of needs EXIF capture times.")
                )
            } else {
                ScrollView {
                    LazyVGrid(columns: columns, spacing: 10) {
                        ForEach(store.bestOfEntries, id: \.item.fileId) { entry in
                            VStack(alignment: .leading, spacing: 4) {
                                ThumbCell(path: entry.item.thumbPath)
                                    .frame(height: 160)
                                Text(Self.label(for: entry))
                                    .font(.caption)
                                Text("best of \(entry.count)")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                            .help(entry.item.path)
                        }
                    }
                    .padding()
                }
            }
        }
        .navigationSubtitle("\(store.bestOfEntries.count) periods")
        .toolbar {
            Picker("Period", selection: $period) {
                ForEach(Period.allCases) { p in
                    Text(p.rawValue).tag(p)
                }
            }
            .pickerStyle(.segmented)
        }
        .task(id: period) {
            await store.loadBestOf(gapSecs: period.gapSecs)
        }
    }

    static func label(for entry: BestOfEntry) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(entry.start))
        return date.formatted(date: .abbreviated, time: .shortened)
    }
}
