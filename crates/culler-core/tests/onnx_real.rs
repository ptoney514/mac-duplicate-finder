//! Integration with the real CLIP ONNX models. Ignored by default (needs
//! the ~600MB download): run scripts/fetch-models.sh, then
//! `cargo test -p culler-core --test onnx_real -- --ignored`.

mod common;

use std::path::PathBuf;

use culler_core::embed::{onnx::OnnxEmbedder, Embedder, EMBED_DIM};

fn models_dir() -> Option<PathBuf> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models");
    repo.join("vision_model.onnx").exists().then_some(repo)
}

#[test]
#[ignore = "requires downloaded models (scripts/fetch-models.sh)"]
fn real_clip_embeds_and_ranks_colors_correctly() {
    let Some(dir) = models_dir() else {
        panic!("models/ not populated; run scripts/fetch-models.sh");
    };
    let embedder = OnnxEmbedder::load(&dir).unwrap();

    let red = common::test_solid(220, 30, 30);
    let blue = common::test_solid(30, 30, 220);
    let red_vec = embedder.embed_image(&red).unwrap();
    let blue_vec = embedder.embed_image(&blue).unwrap();
    let query = embedder.embed_text("a solid red image").unwrap();

    for v in [&red_vec, &blue_vec, &query] {
        assert_eq!(v.len(), EMBED_DIM);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "unit norm, got {norm}");
    }

    let dot = |a: &[f32], b: &[f32]| -> f32 { a.iter().zip(b).map(|(x, y)| x * y).sum() };
    let to_red = dot(&query, &red_vec);
    let to_blue = dot(&query, &blue_vec);
    assert!(
        to_red > to_blue,
        "'red' query must prefer the red image: red={to_red:.3} blue={to_blue:.3}"
    );
}
