import SwiftUI

struct ContentView: View {
    enum SidebarSection: Hashable {
        case library
        case duplicates
        case clusters
        case triage
        case bestOf
        case commit
    }

    @State private var store = EngineStore()
    @State private var selection: SidebarSection = .library
    @State private var showingFolderPicker = false
    @State private var searchText = ""

    var body: some View {
        NavigationSplitView {
            List(selection: $selection) {
                Label("Library", systemImage: "photo.on.rectangle.angled")
                    .tag(SidebarSection.library)
                Label("Duplicates", systemImage: "doc.on.doc")
                    .badge(store.dupeGroups.count)
                    .tag(SidebarSection.duplicates)
                Label("Bursts & Similar", systemImage: "square.stack.3d.down.right")
                    .badge(store.clusterDetails.count)
                    .tag(SidebarSection.clusters)
                Label("Triage", systemImage: "rectangle.stack.badge.play")
                    .tag(SidebarSection.triage)
                Label("Best Of", systemImage: "trophy")
                    .tag(SidebarSection.bestOf)
                Label("Commit", systemImage: "shippingbox")
                    .tag(SidebarSection.commit)
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 200)
        } detail: {
            switch selection {
            case .library:
                LibraryGridView(store: store)
                    .searchable(
                        text: $searchText,
                        placement: .toolbar,
                        prompt: store.modelsReady
                            ? "Search your photos"
                            : "Search (run fetch-models.sh first)"
                    )
                    .onSubmit(of: .search) {
                        Task { await store.search(searchText) }
                    }
                    .onChange(of: searchText) {
                        if searchText.isEmpty {
                            store.clearSearch()
                        }
                    }
            case .duplicates:
                DuplicatesView(store: store)
            case .clusters:
                ClustersView(store: store)
            case .triage:
                TriageView(store: store)
            case .bestOf:
                BestOfView(store: store)
            case .commit:
                CommitView(store: store)
            }
        }
        .navigationTitle("Culler")
        .toolbar {
            ToolbarItemGroup {
                if store.isScanning {
                    ProgressView()
                        .controlSize(.small)
                    Text(store.progressText)
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                }
                Button("Scan Folder…", systemImage: "folder.badge.plus") {
                    showingFolderPicker = true
                }
                .disabled(store.isScanning)
            }
        }
        .fileImporter(
            isPresented: $showingFolderPicker,
            allowedContentTypes: [.folder]
        ) { result in
            if case .success(let url) = result {
                Task { await store.scan(folder: url) }
            }
        }
        .task {
            store.open()
            await store.refresh()
            await store.runFacePass()
            await store.refreshClusters()
        }
        .alert(
            "Something went wrong",
            isPresented: Binding(
                get: { store.errorMessage != nil },
                set: { if !$0 { store.errorMessage = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(store.errorMessage ?? "")
        }
        .frame(minWidth: 800, minHeight: 500)
    }
}
