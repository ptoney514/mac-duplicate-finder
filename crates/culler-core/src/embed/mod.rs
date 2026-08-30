//! CLIP embeddings: 512-dim unit vectors for images and text queries.
//! `Embedder` is a trait so the scan/search plumbing is testable without the
//! ONNX models; `OnnxEmbedder` is the real implementation.

pub mod aesthetic;
pub mod onnx;
pub mod preprocess;

use crate::Result;

/// Embedding dimensionality (CLIP ViT-B/32 projection size, PRD §5.1).
pub const EMBED_DIM: usize = 512;

/// Produces unit-norm [`EMBED_DIM`] vectors. Implementations may block.
pub trait Embedder: Send {
    fn embed_image(&self, img: &image::DynamicImage) -> Result<Vec<f32>>;
    fn embed_text(&self, text: &str) -> Result<Vec<f32>>;
}

/// Scales `v` to unit L2 norm in place (no-op for the zero vector).
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v {
            *x /= norm;
        }
    }
}
