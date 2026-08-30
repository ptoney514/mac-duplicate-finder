import SwiftData
import SwiftUI

@main
struct CullerApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
        .modelContainer(for: [DupeResolution.self, Decision.self])
    }
}
