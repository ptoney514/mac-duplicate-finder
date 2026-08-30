//! LAION aesthetic predictor for CLIP ViT-B/32: a single linear layer over
//! the unit-normalized image embedding. Weights are extracted from LAION's
//! published .pth by scripts/fetch-models.sh into a raw little-endian f32
//! file: 512 weights then 1 bias (2052 bytes).

use std::path::Path;

use crate::embed::EMBED_DIM;
use crate::Result;

pub const AESTHETIC_FILE: &str = "aesthetic_vit_b_32.bin";

pub struct AestheticHead {
    weights: Vec<f32>,
    bias: f32,
}

impl AestheticHead {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|source| crate::CoreError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let expected = (EMBED_DIM + 1) * 4;
        if bytes.len() != expected {
            return Err(crate::CoreError::Model {
                message: format!(
                    "{}: expected {expected} bytes (512 weights + bias), got {}",
                    path.display(),
                    bytes.len()
                ),
            });
        }
        let floats: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        Ok(Self {
            weights: floats[..EMBED_DIM].to_vec(),
            bias: floats[EMBED_DIM],
        })
    }

    /// Raw LAION score, roughly 1..10.
    pub fn raw_score(&self, embedding: &[f32]) -> f32 {
        self.weights
            .iter()
            .zip(embedding)
            .map(|(w, x)| w * x)
            .sum::<f32>()
            + self.bias
    }

    /// Raw score mapped into [0, 1] for the §7 composite.
    pub fn score(&self, embedding: &[f32]) -> f32 {
        ((self.raw_score(embedding) - 1.0) / 9.0).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_raw_weights_and_scores_dot_plus_bias() {
        let dir = std::env::temp_dir().join(format!("culler-aes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(AESTHETIC_FILE);

        // weights = [0.5, -1.0, 0, 0, ...], bias = 2.0
        let mut weights = vec![0.0f32; EMBED_DIM];
        weights[0] = 0.5;
        weights[1] = -1.0;
        let mut bytes: Vec<u8> = weights.iter().flat_map(|f| f.to_le_bytes()).collect();
        bytes.extend_from_slice(&2.0f32.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();

        let head = AestheticHead::load(&path).unwrap();
        let mut embedding = vec![0.0f32; EMBED_DIM];
        embedding[0] = 1.0; // dot = 0.5, raw = 2.5
        assert!((head.raw_score(&embedding) - 2.5).abs() < 1e-6);
        embedding[0] = 0.0;
        embedding[1] = 1.0; // dot = -1.0, raw = 1.0 -> normalized 0
        assert!((head.raw_score(&embedding) - 1.0).abs() < 1e-6);
        assert_eq!(head.score(&embedding), 0.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_wrong_sized_files() {
        let dir = std::env::temp_dir().join(format!("culler-aes-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(AESTHETIC_FILE);
        std::fs::write(&path, [0u8; 100]).unwrap();
        assert!(AestheticHead::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
