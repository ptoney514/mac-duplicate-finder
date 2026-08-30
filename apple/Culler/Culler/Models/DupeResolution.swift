import Foundation
import SwiftData

/// The user's resolution of one exact-duplicate group: which copy to keep.
/// Keyed by the group's BLAKE3 content hash (hex) so it survives file moves
/// (PRD §6). All copies in an exact group share one hash, so the keeper is
/// identified by path. The commit flow (milestone 6) turns resolutions into
/// staged deletions.
@Model
final class DupeResolution {
    @Attribute(.unique) var contentHashHex: String
    var keeperPath: String
    var resolvedAt: Date

    init(contentHashHex: String, keeperPath: String, resolvedAt: Date = .now) {
        self.contentHashHex = contentHashHex
        self.keeperPath = keeperPath
        self.resolvedAt = resolvedAt
    }
}
