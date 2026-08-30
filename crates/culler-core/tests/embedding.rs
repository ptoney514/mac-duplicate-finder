//! CLIP preprocessing and vector helpers (model-free).

mod common;

use culler_core::embed::preprocess::{clip_pixels, CLIP_SIDE};
use culler_core::embed::{normalize, EMBED_DIM};
use image::{DynamicImage, RgbImage};

#[test]
fn clip_pixels_have_model_shape_and_normalized_range() {
    let img = common::test_image(640, 480, true);
    let pixels = clip_pixels(&img);

    assert_eq!(pixels.len(), 3 * (CLIP_SIDE * CLIP_SIDE) as usize);
    assert!(
        pixels.iter().all(|v| (-3.0..=3.0).contains(v)),
        "normalized values stay within a few standard deviations"
    );
    assert_eq!(pixels, clip_pixels(&img), "deterministic");
}

#[test]
fn clip_pixels_center_crop_keeps_the_middle() {
    // 448x224: shortest side is already 224, so the crop takes the middle
    // 224 columns — half black, half white.
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(448, 224, |x, _| {
        if x < 224 {
            image::Rgb([0, 0, 0])
        } else {
            image::Rgb([255, 255, 255])
        }
    }));
    let pixels = clip_pixels(&img);

    // Red plane, top row: left edge of the crop is black, right edge white.
    let first = pixels[0];
    let last = pixels[(CLIP_SIDE - 1) as usize];
    assert!(
        first < -1.0,
        "black normalizes well below zero, got {first}"
    );
    assert!(last > 1.0, "white normalizes well above zero, got {last}");
}

#[test]
fn normalize_produces_unit_vectors_and_ignores_zero() {
    let mut v = vec![0.0f32; EMBED_DIM];
    v[0] = 3.0;
    v[4] = 4.0;
    normalize(&mut v);
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
    assert!((v[0] - 0.6).abs() < 1e-5 && (v[4] - 0.8).abs() < 1e-5);

    let mut zero = vec![0.0f32; 8];
    normalize(&mut zero);
    assert!(zero.iter().all(|x| *x == 0.0));
}
