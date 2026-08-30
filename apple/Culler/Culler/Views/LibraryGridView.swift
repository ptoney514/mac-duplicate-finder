import SwiftUI

/// The library grid: analyzed images, newest capture first, thumbnails
/// loaded lazily from the engine's on-disk cache.
struct LibraryGridView: View {
    let store: EngineStore

    private let columns = [
        GridItem(.adaptive(minimum: 128, maximum: 192), spacing: 8)
    ]

    var body: some View {
        Group {
            if let results = store.searchResults {
                searchGrid(results)
            } else if store.libraryItems.isEmpty {
                ContentUnavailableView(
                    "No Images Yet",
                    systemImage: "photo.on.rectangle.angled",
                    description: Text("Scan a folder to index your library.")
                )
            } else {
                ScrollView {
                    LazyVGrid(columns: columns, spacing: 8) {
                        ForEach(store.libraryItems, id: \.fileId) { item in
                            ThumbCell(path: item.thumbPath)
                                .help(item.path)
                        }
                    }
                    .padding()
                }
            }
        }
        .navigationSubtitle(subtitle)
    }

    private var subtitle: String {
        if let results = store.searchResults {
            "\(results.count) search results"
        } else {
            "\(store.libraryItems.count) images"
        }
    }

    @ViewBuilder
    private func searchGrid(_ results: [SearchResult]) -> some View {
        if results.isEmpty {
            ContentUnavailableView.search
        } else {
            ScrollView {
                LazyVGrid(columns: columns, spacing: 8) {
                    ForEach(results, id: \.fileId) { hit in
                        ThumbCell(path: hit.thumbPath)
                            .overlay(alignment: .bottomTrailing) {
                                Text(String(format: "%.2f", hit.score))
                                    .font(.caption2.monospacedDigit())
                                    .padding(3)
                                    .background(.black.opacity(0.6), in: RoundedRectangle(cornerRadius: 4))
                                    .foregroundStyle(.white)
                                    .padding(4)
                            }
                            .help(hit.path)
                    }
                }
                .padding()
            }
        }
    }
}

/// Loads one cached thumbnail off the main actor (file Data is Sendable;
/// NSImage is constructed back on main).
struct ThumbCell: View {
    let path: String?
    @State private var image: NSImage?

    var body: some View {
        ZStack {
            Rectangle()
                .fill(.quaternary)
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFill()
            } else {
                Image(systemName: "photo")
                    .foregroundStyle(.tertiary)
            }
        }
        .aspectRatio(1, contentMode: .fill)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .task(id: path) {
            guard image == nil, let path else { return }
            let data = await Task.detached(priority: .utility) {
                try? Data(contentsOf: URL(fileURLWithPath: path))
            }.value
            if let data {
                image = NSImage(data: data)
            }
        }
    }
}
