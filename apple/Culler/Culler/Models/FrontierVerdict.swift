import Foundation
import SwiftData

/// Cached frontier verdict, keyed by a hash of the cluster's member content
/// hashes so a cluster is never sent twice (§5.3) and the cache survives
/// recluster runs (stored cluster ids regenerate; ADR-0006).
@Model
final class FrontierVerdict {
    @Attribute(.unique) var clusterHashHex: String
    var model: String
    var keeperHashHex: String
    var reason: String
    var createdAt: Date

    init(clusterHashHex: String, model: String, keeperHashHex: String, reason: String) {
        self.clusterHashHex = clusterHashHex
        self.model = model
        self.keeperHashHex = keeperHashHex
        self.reason = reason
        self.createdAt = .now
    }
}
