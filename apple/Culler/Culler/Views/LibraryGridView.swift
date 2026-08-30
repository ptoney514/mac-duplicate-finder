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
            if store.libraryItems.isEmpty {
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
        .navigationSubtitle("\(store.libraryItems.count) images")
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
