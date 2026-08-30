//! Composite quality scoring within a cluster (PRD §7). Signals missing for
//! a whole cluster (e.g. no aesthetic model, no face pass yet) drop out and
//! the remaining weights renormalize; signals missing for one member score
//! neutral (0.5).

/// Configurable §7 weights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityWeights {
    pub sharpness: f64,
    pub faces: f64,
    pub aesthetic: f64,
    pub exposure: f64,
    pub resolution: f64,
}

impl Default for QualityWeights {
    fn default() -> Self {
        Self {
            sharpness: 0.30,
            faces: 0.25,
            aesthetic: 0.25,
            exposure: 0.15,
            resolution: 0.05,
        }
    }
}

/// Stored signals for one cluster member.
#[derive(Debug, Clone, Default)]
pub struct MemberSignals {
    pub id: i64,
    pub sharpness: Option<f64>,
    /// Already in [0, 1].
    pub exposure: Option<f64>,
    /// Already in [0, 1].
    pub aesthetic: Option<f64>,
    pub face_count: Option<i64>,
    pub eyes_open_ratio: Option<f64>,
    /// width * height.
    pub pixels: Option<u64>,
}

/// Composite score in [0, 1] per member, in input order. Sharpness is
/// normalized as a ratio to the cluster's sharpest member (min-max would
/// blow a 10% sharpness edge up to a full weight); resolution is relative
/// to the largest member; the faces signal only participates when the
/// cluster contains faces (then eyes-open ratio drives it).
pub fn composite_scores(members: &[MemberSignals], weights: &QualityWeights) -> Vec<f64> {
    if members.is_empty() {
        return Vec::new();
    }

    let max_sharpness = members
        .iter()
        .filter_map(|m| m.sharpness)
        .fold(0.0f64, f64::max);
    let max_pixels = members.iter().filter_map(|m| m.pixels).max().unwrap_or(0);

    let sharpness_active = members.iter().any(|m| m.sharpness.is_some()) && max_sharpness > 1e-12;
    let faces_active = members.iter().any(|m| m.face_count.is_some_and(|c| c > 0));
    let aesthetic_active = members.iter().any(|m| m.aesthetic.is_some());
    let exposure_active = members.iter().any(|m| m.exposure.is_some());
    let resolution_active = max_pixels > 0;

    let mut weight_sum = 0.0;
    if sharpness_active {
        weight_sum += weights.sharpness;
    }
    if faces_active {
        weight_sum += weights.faces;
    }
    if aesthetic_active {
        weight_sum += weights.aesthetic;
    }
    if exposure_active {
        weight_sum += weights.exposure;
    }
    if resolution_active {
        weight_sum += weights.resolution;
    }
    if weight_sum <= 0.0 {
        return vec![0.5; members.len()];
    }

    members
        .iter()
        .map(|m| {
            let mut score = 0.0;
            if sharpness_active {
                let signal = m
                    .sharpness
                    .map_or(0.5, |s| (s / max_sharpness).clamp(0.0, 1.0));
                score += weights.sharpness * signal;
            }
            if faces_active {
                let signal = if m.face_count.unwrap_or(0) > 0 {
                    m.eyes_open_ratio.unwrap_or(0.5).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                score += weights.faces * signal;
            }
            if aesthetic_active {
                score += weights.aesthetic * m.aesthetic.unwrap_or(0.5).clamp(0.0, 1.0);
            }
            if exposure_active {
                score += weights.exposure * m.exposure.unwrap_or(0.5).clamp(0.0, 1.0);
            }
            if resolution_active {
                let signal = m.pixels.map_or(0.5, |p| p as f64 / max_pixels as f64);
                score += weights.resolution * signal;
            }
            (score / weight_sum).clamp(0.0, 1.0)
        })
        .collect()
}

/// Index of the proposed keeper: highest composite, ties to the smaller id.
pub fn keeper_index(members: &[MemberSignals], weights: &QualityWeights) -> Option<usize> {
    let scores = composite_scores(members, weights);
    let mut best: Option<usize> = None;
    for (i, score) in scores.iter().enumerate() {
        let better = match best {
            None => true,
            Some(b) => {
                *score > scores[b] + f64::EPSILON
                    || ((*score - scores[b]).abs() <= f64::EPSILON && members[i].id < members[b].id)
            }
        };
        if better {
            best = Some(i);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: i64) -> MemberSignals {
        MemberSignals {
            id,
            sharpness: Some(100.0),
            exposure: Some(0.9),
            aesthetic: Some(0.5),
            face_count: Some(0),
            eyes_open_ratio: None,
            pixels: Some(12_000_000),
        }
    }

    #[test]
    fn sharpest_member_wins_when_everything_else_ties() {
        let mut a = member(1);
        let mut b = member(2);
        a.sharpness = Some(50.0);
        b.sharpness = Some(400.0);
        let members = [a, b];
        let scores = composite_scores(&members, &QualityWeights::default());
        assert!(scores[1] > scores[0]);
        assert_eq!(keeper_index(&members, &QualityWeights::default()), Some(1));
    }

    #[test]
    fn eyes_open_beats_eyes_closed_in_a_faces_cluster() {
        let mut open = member(1);
        let mut closed = member(2);
        open.face_count = Some(2);
        open.eyes_open_ratio = Some(1.0);
        closed.face_count = Some(2);
        closed.eyes_open_ratio = Some(0.0);
        // Slightly sharper closed-eyes shot: faces weight must still win.
        closed.sharpness = Some(110.0);
        let members = [open, closed];
        assert_eq!(keeper_index(&members, &QualityWeights::default()), Some(0));
    }

    #[test]
    fn faceless_clusters_ignore_the_faces_weight() {
        let mut a = member(1);
        let mut b = member(2);
        a.face_count = Some(0);
        b.face_count = Some(0);
        b.exposure = Some(1.0); // only differentiator
        let members = [a, b];
        let scores = composite_scores(&members, &QualityWeights::default());
        assert!(scores[1] > scores[0]);
    }

    #[test]
    fn resolution_bonus_breaks_ties() {
        let mut small = member(1);
        let mut large = member(2);
        small.pixels = Some(3_000_000);
        large.pixels = Some(24_000_000);
        let members = [small, large];
        assert_eq!(keeper_index(&members, &QualityWeights::default()), Some(1));
    }

    #[test]
    fn custom_weights_isolate_a_signal() {
        let weights = QualityWeights {
            sharpness: 0.0,
            faces: 0.0,
            aesthetic: 1.0,
            exposure: 0.0,
            resolution: 0.0,
        };
        let mut ugly = member(1);
        let mut pretty = member(2);
        ugly.aesthetic = Some(0.2);
        pretty.aesthetic = Some(0.9);
        // Everything else favors `ugly`.
        ugly.sharpness = Some(500.0);
        ugly.exposure = Some(1.0);
        let members = [ugly, pretty];
        assert_eq!(keeper_index(&members, &weights), Some(1));
    }

    #[test]
    fn missing_signals_fall_back_to_neutral_and_scores_stay_in_range() {
        let members = [
            MemberSignals {
                id: 1,
                ..Default::default()
            },
            member(2),
        ];
        let scores = composite_scores(&members, &QualityWeights::default());
        assert_eq!(scores.len(), 2);
        for s in &scores {
            assert!((0.0..=1.0).contains(s), "score out of range: {s}");
        }
    }

    #[test]
    fn equal_members_tie_at_half_and_keeper_takes_smaller_id() {
        let members = [member(5), member(3)];
        let scores = composite_scores(&members, &QualityWeights::default());
        assert!((scores[0] - scores[1]).abs() < 1e-9);
        assert_eq!(keeper_index(&members, &QualityWeights::default()), Some(1));
    }
}
