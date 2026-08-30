//! Behavioral tests for dHash/pHash: invariant under re-encoding, stable
//! under small tonal shifts and resizes, far apart for different structure.

mod common;

use common::test_image;
use culler_core::analyze::phash::{dhash, hamming, phash};
use culler_core::cluster::near::{DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX};

#[test]
fn hamming_counts_differing_bits() {
    assert_eq!(hamming(0, 0), 0);
    assert_eq!(hamming(0, u64::MAX), 64);
    assert_eq!(hamming(0b1011, 0b0010), 2);
    assert_eq!(hamming(1 << 63, 0), 1);
}

#[test]
fn identical_images_have_identical_hashes() {
    let a = test_image(320, 200, true);
    let b = test_image(320, 200, true);
    assert_eq!(dhash(&a), dhash(&b));
    assert_eq!(phash(&a), phash(&b));
}

#[test]
fn brightness_shift_stays_within_near_thresholds() {
    let base = test_image(320, 200, true);
    let bright = base.brighten(14);
    assert!(
        hamming(dhash(&base), dhash(&bright)) <= DEFAULT_DHASH_MAX,
        "dhash distance {}",
        hamming(dhash(&base), dhash(&bright))
    );
    assert!(
        hamming(phash(&base), phash(&bright)) <= DEFAULT_PHASH_MAX,
        "phash distance {}",
        hamming(phash(&base), phash(&bright))
    );
}

#[test]
fn resized_copy_stays_within_near_thresholds() {
    let base = test_image(640, 400, true);
    let smaller = base.resize(320, 200, image::imageops::FilterType::Triangle);
    assert!(hamming(dhash(&base), dhash(&smaller)) <= DEFAULT_DHASH_MAX);
    assert!(hamming(phash(&base), phash(&smaller)) <= DEFAULT_PHASH_MAX);
}

#[test]
fn structurally_different_images_exceed_thresholds() {
    let horizontal = test_image(320, 200, true);
    let vertical = test_image(320, 200, false);
    assert!(
        hamming(dhash(&horizontal), dhash(&vertical)) > DEFAULT_DHASH_MAX,
        "dhash distance {}",
        hamming(dhash(&horizontal), dhash(&vertical))
    );
    assert!(
        hamming(phash(&horizontal), phash(&vertical)) > DEFAULT_PHASH_MAX,
        "phash distance {}",
        hamming(phash(&horizontal), phash(&vertical))
    );
}
