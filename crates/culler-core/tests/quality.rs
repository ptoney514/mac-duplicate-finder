//! Sharpness and exposure signals: behavioral tests plus their persistence
//! through the analysis pass.

mod common;

use common::{images_row, open_engine, save_jpeg, scan, test_image, TempDir};
use culler_core::analyze::quality::{exposure_score, sharpness};
use image::{DynamicImage, RgbImage};

fn checkerboard(square: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(512, 512, |x, y| {
        if ((x / square) + (y / square)).is_multiple_of(2) {
            image::Rgb([245, 245, 245])
        } else {
            image::Rgb([10, 10, 10])
        }
    }))
}

fn solid(l: u8) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(256, 256, image::Rgb([l, l, l])))
}

#[test]
fn sharp_edges_score_higher_than_their_blur() {
    let sharp = checkerboard(8);
    let blurred = sharp.blur(6.0);
    let s_sharp = sharpness(&sharp);
    let s_blur = sharpness(&blurred);
    assert!(
        s_sharp > s_blur * 3.0,
        "sharp {s_sharp} should dwarf blurred {s_blur}"
    );
    assert!(sharpness(&solid(128)) < 1e-6, "flat image has no gradients");
}

#[test]
fn exposure_penalizes_clipped_histograms() {
    assert!(exposure_score(&solid(128)) > 0.99, "mid-gray is fine");
    assert!(exposure_score(&solid(255)) < 0.01, "fully blown");
    assert!(exposure_score(&solid(0)) < 0.01, "fully crushed");

    // Half well-exposed, half blown: score near 0.5.
    let half = DynamicImage::ImageRgb8(RgbImage::from_fn(256, 256, |x, _| {
        if x < 128 {
            image::Rgb([128, 128, 128])
        } else {
            image::Rgb([255, 255, 255])
        }
    }));
    let score = exposure_score(&half);
    assert!((0.4..=0.6).contains(&score), "got {score}");
}

#[test]
fn analysis_persists_quality_signals() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let photo = save_jpeg(&test_image(320, 200, true), &lib, "photo.jpg");
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);

    let row = images_row(dir.path(), &photo).unwrap();
    assert!(row.sharpness.is_some(), "sharpness stored");
    let exposure = row.exposure_score.expect("exposure stored");
    assert!((0.0..=1.0).contains(&exposure));
}
