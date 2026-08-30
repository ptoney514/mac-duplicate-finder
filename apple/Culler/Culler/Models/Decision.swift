import Foundation
import SwiftData

/// A per-image verdict (keep / reject / maybe), keyed by content hash so it
/// survives file moves (PRD §6). Written by the burst resolver now and the
/// triage mode in milestone 6; consumed by the commit flow.
@Model
final class Decision {
    @Attribute(.unique) var contentHashHex: String
    var stateRaw: String
    var decidedAt: Date

    enum State: String {
        case keep, reject, maybe
    }

    var state: State {
        get { State(rawValue: stateRaw) ?? .maybe }
        set { stateRaw = newValue.rawValue }
    }

    init(contentHashHex: String, state: State, decidedAt: Date = .now) {
        self.contentHashHex = contentHashHex
        self.stateRaw = state.rawValue
        self.decidedAt = decidedAt
    }
}
