//! Burst detection (PRD §8): frames captured within 3 seconds of each other
//! on the same camera whose embeddings are cosine-similar (>= 0.92). Frames
//! chain: A~B and B~C form one burst even if A and C are farther apart.

/// Defaults from PRD §8.
pub const DEFAULT_BURST_GAP_SECS: i64 = 3;
pub const DEFAULT_BURST_MIN_COSINE: f32 = 0.92;

/// One candidate frame. Frames missing camera, capture time, or embedding
/// can never join a burst.
#[derive(Debug, Clone)]
pub struct BurstFrame {
    pub id: i64,
    pub camera: Option<String>,
    pub captured_at: Option<i64>,
    pub embedding: Option<Vec<f32>>,
}

/// Groups frames into bursts. Only components with two or more members are
/// returned; ids sorted within, components by size descending then min id.
pub fn burst_components(
    frames: &[BurstFrame],
    max_gap_secs: i64,
    min_cosine: f32,
) -> Vec<Vec<i64>> {
    use std::collections::HashMap;

    let mut by_camera: HashMap<&str, Vec<&BurstFrame>> = HashMap::new();
    for frame in frames {
        if let (Some(camera), Some(_), Some(_)) =
            (&frame.camera, frame.captured_at, frame.embedding.as_ref())
        {
            by_camera.entry(camera).or_default().push(frame);
        }
    }

    let cosine = |a: &[f32], b: &[f32]| -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if na == 0.0 || nb == 0.0 {
            0.0
        } else {
            dot / (na * nb)
        }
    };

    let mut components: Vec<Vec<i64>> = Vec::new();
    for (_camera, mut group) in by_camera {
        group.sort_by_key(|f| (f.captured_at.unwrap(), f.id));
        let mut current = vec![group[0].id];
        for pair in group.windows(2) {
            let (prev, cur) = (pair[0], pair[1]);
            let gap = cur.captured_at.unwrap() - prev.captured_at.unwrap();
            let similar = cosine(
                prev.embedding.as_ref().unwrap(),
                cur.embedding.as_ref().unwrap(),
            ) >= min_cosine;
            if gap <= max_gap_secs && similar {
                current.push(cur.id);
            } else {
                if current.len() >= 2 {
                    components.push(std::mem::take(&mut current));
                }
                current = vec![cur.id];
            }
        }
        if current.len() >= 2 {
            components.push(current);
        }
    }

    for component in &mut components {
        component.sort_unstable();
    }
    components.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));
    components
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{normalize, EMBED_DIM};

    fn vec_along(basis: usize, lean: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; EMBED_DIM];
        v[basis] = 1.0;
        v[(basis + 1) % EMBED_DIM] = lean;
        normalize(&mut v);
        v
    }

    fn frame(id: i64, camera: &str, at: i64, embedding: Vec<f32>) -> BurstFrame {
        BurstFrame {
            id,
            camera: Some(camera.to_owned()),
            captured_at: Some(at),
            embedding: Some(embedding),
        }
    }

    #[test]
    fn consecutive_similar_frames_chain_into_one_burst() {
        // 4 frames 2s apart: first and last are 6s apart but chain through.
        let frames: Vec<BurstFrame> = (0..4)
            .map(|i| frame(i, "iPhone", 1000 + i * 2, vec_along(0, 0.1 * i as f32)))
            .collect();
        let bursts = burst_components(&frames, DEFAULT_BURST_GAP_SECS, DEFAULT_BURST_MIN_COSINE);
        assert_eq!(bursts, vec![vec![0, 1, 2, 3]]);
    }

    #[test]
    fn time_gap_splits_bursts() {
        let frames = [
            frame(1, "iPhone", 1000, vec_along(0, 0.0)),
            frame(2, "iPhone", 1002, vec_along(0, 0.05)),
            frame(3, "iPhone", 1010, vec_along(0, 0.1)), // 8s later
            frame(4, "iPhone", 1012, vec_along(0, 0.15)),
        ];
        let bursts = burst_components(&frames, DEFAULT_BURST_GAP_SECS, DEFAULT_BURST_MIN_COSINE);
        assert_eq!(bursts.len(), 2);
        assert!(bursts.contains(&vec![1, 2]) && bursts.contains(&vec![3, 4]));
    }

    #[test]
    fn different_cameras_never_share_a_burst() {
        let frames = [
            frame(1, "iPhone", 1000, vec_along(0, 0.0)),
            frame(2, "Canon", 1001, vec_along(0, 0.0)),
        ];
        assert!(
            burst_components(&frames, DEFAULT_BURST_GAP_SECS, DEFAULT_BURST_MIN_COSINE).is_empty()
        );
    }

    #[test]
    fn dissimilar_content_splits_even_within_the_gap() {
        // Orthogonal embeddings: cosine 0 < 0.92.
        let frames = [
            frame(1, "iPhone", 1000, vec_along(0, 0.0)),
            frame(2, "iPhone", 1001, vec_along(7, 0.0)),
        ];
        assert!(
            burst_components(&frames, DEFAULT_BURST_GAP_SECS, DEFAULT_BURST_MIN_COSINE).is_empty()
        );
    }

    #[test]
    fn cosine_threshold_is_tunable() {
        let frames = [
            frame(1, "iPhone", 1000, vec_along(0, 0.0)),
            frame(2, "iPhone", 1001, vec_along(7, 0.0)),
        ];
        // With the bar on the floor, time alone groups them.
        let bursts = burst_components(&frames, DEFAULT_BURST_GAP_SECS, -1.0);
        assert_eq!(bursts, vec![vec![1, 2]]);
    }

    #[test]
    fn incomplete_frames_are_ignored() {
        let frames = [
            frame(1, "iPhone", 1000, vec_along(0, 0.0)),
            BurstFrame {
                id: 2,
                camera: None,
                captured_at: Some(1001),
                embedding: Some(vec_along(0, 0.0)),
            },
            BurstFrame {
                id: 3,
                camera: Some("iPhone".into()),
                captured_at: None,
                embedding: Some(vec_along(0, 0.0)),
            },
            BurstFrame {
                id: 4,
                camera: Some("iPhone".into()),
                captured_at: Some(1001),
                embedding: None,
            },
        ];
        assert!(
            burst_components(&frames, DEFAULT_BURST_GAP_SECS, DEFAULT_BURST_MIN_COSINE).is_empty()
        );
    }
}
