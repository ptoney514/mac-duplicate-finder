//! Per-image analysis: EXIF facts, cached thumbnails, perceptual hashes.
//! Quality signals (sharpness, exposure, faces, aesthetics) arrive in
//! milestone 5.

pub mod exif;
pub mod phash;
pub mod quality;
pub mod thumbs;

use std::path::Path;

/// Everything analysis learns about one image; one `images` row.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ImageFacts {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub captured_at: Option<i64>,
    pub camera: Option<String>,
    pub orientation: Option<u16>,
    pub dhash: Option<u64>,
    pub phash: Option<u64>,
    pub thumb_path: Option<String>,
    pub sharpness: Option<f64>,
    pub exposure_score: Option<f64>,
}

/// Analyzes one file: EXIF (best effort), then pixel-derived facts if the
/// format is decodable (dimensions, dHash, pHash, cached thumbnail keyed by
/// `content_hash` under `thumbs_dir`). Undecodable files still produce a row
/// with whatever EXIF gave us. The bool reports whether a non-fatal problem
/// occurred (decode or thumbnail failure).
pub fn analyze_file(path: &Path, content_hash: &[u8; 32], thumbs_dir: &Path) -> (ImageFacts, bool) {
    let mut facts = ImageFacts::default();
    let mut soft_error = false;

    let exif_facts = exif::read_exif(path);
    facts.captured_at = exif_facts.captured_at;
    facts.camera = exif_facts.camera;
    facts.orientation = exif_facts.orientation;

    match image::open(path) {
        Ok(img) => {
            facts.width = Some(img.width());
            facts.height = Some(img.height());
            facts.dhash = Some(phash::dhash(&img));
            facts.phash = Some(phash::phash(&img));
            facts.sharpness = Some(quality::sharpness(&img));
            facts.exposure_score = Some(quality::exposure_score(&img));
            match thumbs::ensure_thumb(&img, content_hash, thumbs_dir) {
                Ok(p) => facts.thumb_path = Some(p.to_string_lossy().into_owned()),
                Err(_) => soft_error = true,
            }
        }
        // HEIC/RAW (ADR-0002) and corrupt files: keep the EXIF-only row.
        Err(_) => soft_error = true,
    }
    (facts, soft_error)
}
