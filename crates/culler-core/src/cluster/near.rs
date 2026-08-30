//! Near-duplicate clustering: connected components of the "dHash distance
//! <= t1 OR pHash distance <= t2" relation (PRD section 8).
//!
//! Candidate pairs come from Hamming-LSH banding: a 64-bit hash is split into
//! `max_distance + 1` bands, and by pigeonhole two hashes within the distance
//! bound must agree exactly on at least one band. Only bucket collisions get
//! a real distance check, so the pass never does an all-pairs sweep.

use std::collections::{HashMap, HashSet};

/// Default thresholds from PRD section 8.
pub const DEFAULT_DHASH_MAX: u32 = 8;
pub const DEFAULT_PHASH_MAX: u32 = 10;

/// One analyzed image's perceptual hashes.
#[derive(Debug, Clone, Copy)]
pub struct HashedImage {
    pub id: i64,
    pub dhash: Option<u64>,
    pub phash: Option<u64>,
}

/// Groups `items` into near-duplicate components. Only components with two
/// or more members are returned; member ids are sorted within each component
/// and components are sorted by size descending, then smallest id.
pub fn near_components(items: &[HashedImage], dhash_max: u32, phash_max: u32) -> Vec<Vec<i64>> {
    use crate::analyze::phash::hamming;

    type HashGetter = fn(&HashedImage) -> Option<u64>;
    let mut uf = UnionFind::new(items.len());
    let hash_kinds: [(HashGetter, u32); 2] =
        [(|it| it.dhash, dhash_max), (|it| it.phash, phash_max)];
    for (get, max) in hash_kinds {
        let hashes: Vec<(usize, u64)> = items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| get(it).map(|h| (i, h)))
            .collect();
        for (i, j) in candidate_pairs(&hashes, max) {
            if hamming(get(&items[i]).unwrap(), get(&items[j]).unwrap()) <= max {
                uf.union(i, j);
            }
        }
    }

    let mut by_root: HashMap<usize, Vec<i64>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        by_root.entry(uf.find(i)).or_default().push(item.id);
    }
    let mut components: Vec<Vec<i64>> = by_root
        .into_values()
        .filter(|c| c.len() >= 2)
        .map(|mut c| {
            c.sort_unstable();
            c
        })
        .collect();
    components.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));
    components
}

/// Indices of `hashes` pairs that *might* be within `max_distance` (superset
/// of the true pairs; caller verifies). Pairs are (smaller, larger) indices.
/// With `max_distance + 1` bands, pigeonhole guarantees no true pair is
/// missed: two hashes differing in at most `max_distance` bits must agree
/// exactly on at least one band.
fn candidate_pairs(hashes: &[(usize, u64)], max_distance: u32) -> HashSet<(usize, usize)> {
    let bands = u64::from(max_distance + 1).min(64);
    let mut pairs = HashSet::new();
    let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
    for band in 0..bands {
        let start = (band * 64 / bands) as u32;
        let end = ((band + 1) * 64 / bands) as u32;
        let width = end - start;
        let mask = if width == 64 {
            u64::MAX
        } else {
            ((1u64 << width) - 1) << start
        };
        buckets.clear();
        for &(idx, hash) in hashes {
            buckets.entry((hash & mask) >> start).or_default().push(idx);
        }
        for members in buckets.values() {
            for a in 0..members.len() {
                for b in (a + 1)..members.len() {
                    let (i, j) = (members[a].min(members[b]), members[a].max(members[b]));
                    pairs.insert((i, j));
                }
            }
        }
    }
    pairs
}

struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            size: vec![1; n],
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (mut ra, mut rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        if self.size[ra] < self.size[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        self.size[ra] += self.size[rb];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::phash::hamming;

    /// Deterministic xorshift so the property test needs no rand crate.
    fn random_hashes(n: usize, mut seed: u64) -> Vec<u64> {
        (0..n)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed
            })
            .collect()
    }

    fn flip_bits(base: u64, bits: &[u32]) -> u64 {
        bits.iter().fold(base, |h, b| h ^ (1u64 << b))
    }

    #[test]
    fn banding_finds_every_pair_within_the_bound() {
        // Random hashes plus planted near-pairs at and around the threshold.
        let mut hashes = random_hashes(300, 0xC0FFEE);
        let base = hashes[0];
        hashes.push(flip_bits(base, &[1, 5, 9, 13, 17, 21, 25, 29, 33, 37])); // d=10
        hashes.push(flip_bits(base, &[2, 6, 10])); // d=3
        let indexed: Vec<(usize, u64)> = hashes.iter().copied().enumerate().collect();

        let max = 10;
        let candidates = candidate_pairs(&indexed, max);
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                if hamming(hashes[i], hashes[j]) <= max {
                    assert!(
                        candidates.contains(&(i, j)),
                        "banding missed pair ({i},{j}) at distance {}",
                        hamming(hashes[i], hashes[j])
                    );
                }
            }
        }
    }

    fn img(id: i64, dhash: u64, phash: u64) -> HashedImage {
        HashedImage {
            id,
            dhash: Some(dhash),
            phash: Some(phash),
        }
    }

    #[test]
    fn clusters_at_threshold_but_not_beyond() {
        let far = 0xFFFF_FFFF_FFFF_FFFFu64;
        let items = [
            img(1, 0, far),
            // dhash distance exactly 8 from id 1, phash far: in.
            img(
                2,
                flip_bits(0, &[0, 1, 2, 3, 4, 5, 6, 7]),
                0x00FF_FF00_0000_0000,
            ),
            // dhash distance 9, phash far from everyone: out.
            img(
                3,
                flip_bits(0, &[10, 11, 12, 13, 14, 15, 16, 17, 18]),
                0xF0F0_0F0F_AAAA_5555,
            ),
        ];
        let components = near_components(&items, DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX);
        assert_eq!(components, vec![vec![1, 2]]);
    }

    #[test]
    fn phash_within_bound_is_enough_on_its_own() {
        let items = [
            img(7, 0, 0xABCD_0000_0000_0000),
            // dhash far (32 bits), phash distance exactly 10: in.
            img(
                8,
                0xFFFF_FFFF_0000_0000,
                flip_bits(0xABCD_0000_0000_0000, &[0, 2, 4, 6, 8, 10, 12, 14, 16, 18]),
            ),
        ];
        let components = near_components(&items, DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX);
        assert_eq!(components, vec![vec![7, 8]]);

        // Distance 11 phash and far dhash: nothing clusters.
        let items = [
            img(7, 0, 0xABCD_0000_0000_0000),
            img(
                8,
                0xFFFF_FFFF_0000_0000,
                flip_bits(
                    0xABCD_0000_0000_0000,
                    &[0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20],
                ),
            ),
        ];
        assert!(near_components(&items, DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX).is_empty());
    }

    #[test]
    fn chains_merge_transitively_into_one_component() {
        // a~b and b~c but a is far from c: still one cluster of three.
        let a = 0u64;
        let b = flip_bits(a, &[0, 1, 2, 3, 4, 5]); // d(a,b)=6
        let c = flip_bits(b, &[8, 9, 10, 11, 12, 13]); // d(b,c)=6, d(a,c)=12
        let far = 0xFFFF_FFFF_FFFF_FFFFu64;
        let items = [img(1, a, far), img(2, b, far ^ 1), img(3, c, far ^ 3)];
        let components = near_components(&items, DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX);
        assert_eq!(components, vec![vec![1, 2, 3]]);
    }

    #[test]
    fn missing_hashes_never_cluster() {
        let items = [
            HashedImage {
                id: 1,
                dhash: None,
                phash: None,
            },
            HashedImage {
                id: 2,
                dhash: None,
                phash: None,
            },
            img(3, 5, 5),
        ];
        assert!(near_components(&items, DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX).is_empty());
    }
}
