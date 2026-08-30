//! Nearest-neighbor index over embeddings (usearch, cosine metric).

use std::path::Path;

use usearch::{IndexOptions, MetricKind, ScalarKind};

use crate::embed::EMBED_DIM;
use crate::{CoreError, Result};

fn model_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Model {
        message: e.to_string(),
    }
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| CoreError::Model {
        message: format!("non-UTF8 index path: {}", path.display()),
    })
}

fn make_index() -> Result<usearch::Index> {
    let options = IndexOptions {
        dimensions: EMBED_DIM,
        metric: MetricKind::Cos,
        quantization: ScalarKind::F32,
        ..Default::default()
    };
    usearch::Index::new(&options).map_err(model_err)
}

pub struct VectorIndex {
    inner: usearch::Index,
}

impl VectorIndex {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: make_index()?,
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let index = make_index()?;
        index.load(path_str(path)?).map_err(model_err)?;
        Ok(Self { inner: index })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.inner.save(path_str(path)?).map_err(model_err)
    }

    pub fn len(&self) -> usize {
        self.inner.size()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn add(&mut self, key: u64, vector: &[f32]) -> Result<()> {
        if self.inner.capacity() <= self.inner.size() {
            let target = (self.inner.size() + 1).next_power_of_two().max(64);
            self.inner.reserve(target).map_err(model_err)?;
        }
        self.inner.add(key, vector).map_err(model_err)
    }

    /// Nearest neighbors as (key, cosine similarity), best first.
    pub fn search(&self, vector: &[f32], k: usize) -> Result<Vec<(u64, f32)>> {
        let matches = self.inner.search(vector, k).map_err(model_err)?;
        Ok(matches
            .keys
            .into_iter()
            .zip(matches.distances)
            .map(|(key, distance)| (key, 1.0 - distance))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{normalize, EMBED_DIM};

    fn unit(basis: usize, lean: f32) -> Vec<f32> {
        // A vector mostly along `basis` with a small lean onto the next axis.
        let mut v = vec![0.0f32; EMBED_DIM];
        v[basis] = 1.0;
        v[(basis + 1) % EMBED_DIM] = lean;
        normalize(&mut v);
        v
    }

    #[test]
    fn nearest_neighbors_rank_by_cosine_similarity() {
        let mut index = VectorIndex::new().unwrap();
        index.add(10, &unit(0, 0.0)).unwrap();
        index.add(20, &unit(0, 0.4)).unwrap();
        index.add(30, &unit(5, 0.0)).unwrap();

        let hits = index.search(&unit(0, 0.1), 3).unwrap();
        let keys: Vec<u64> = hits.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, [10, 20, 30]);
        assert!(hits[0].1 > hits[1].1 && hits[1].1 > hits[2].1);
        assert!(hits[0].1 > 0.99, "near-identical vectors score ~1");
        assert!(hits[2].1 < 0.2, "orthogonal vectors score ~0");
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("culler-index-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.usearch");

        let mut index = VectorIndex::new().unwrap();
        index.add(7, &unit(3, 0.0)).unwrap();
        index.add(8, &unit(9, 0.0)).unwrap();
        index.save(&path).unwrap();

        let loaded = VectorIndex::load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        let hits = loaded.search(&unit(3, 0.0), 1).unwrap();
        assert_eq!(hits[0].0, 7);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
