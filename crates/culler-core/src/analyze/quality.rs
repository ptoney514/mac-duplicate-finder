//! Pixel-derived quality signals (PRD §7): sharpness via variance of the
//! Laplacian on a grayscale downscale, exposure via histogram clipping.

use image::DynamicImage;

/// Long edge of the grayscale downscale used for the Laplacian.
const SHARPNESS_SIDE: u32 = 256;

/// Variance of the 3x3 Laplacian over a grayscale downscale. Unitless and
/// only comparable within a cluster (PRD §7 normalizes it there).
pub fn sharpness(img: &DynamicImage) -> f64 {
    let gray = if img.width().max(img.height()) > SHARPNESS_SIDE {
        img.resize(
            SHARPNESS_SIDE,
            SHARPNESS_SIDE,
            image::imageops::FilterType::Triangle,
        )
        .to_luma8()
    } else {
        img.to_luma8()
    };
    let (w, h) = gray.dimensions();
    if w < 3 || h < 3 {
        return 0.0;
    }
    let px = |x: u32, y: u32| gray.get_pixel(x, y)[0] as f64 / 255.0;
    let (mut sum, mut sum_sq, mut n) = (0.0, 0.0, 0.0);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let lap = px(x + 1, y) + px(x - 1, y) + px(x, y + 1) + px(x, y - 1) - 4.0 * px(x, y);
            sum += lap;
            sum_sq += lap * lap;
            n += 1.0;
        }
    }
    let mean = sum / n;
    (sum_sq / n - mean * mean).max(0.0)
}

/// Exposure score in [0, 1]: 1 minus the fraction of pixels in the crushed
/// (<= 2) or blown (>= 253) luma bins.
pub fn exposure_score(img: &DynamicImage) -> f64 {
    let gray = img.thumbnail(SHARPNESS_SIDE, SHARPNESS_SIDE).to_luma8();
    let total = (gray.width() * gray.height()) as f64;
    if total == 0.0 {
        return 0.0;
    }
    let clipped = gray.pixels().filter(|p| p[0] <= 2 || p[0] >= 253).count() as f64;
    (1.0 - clipped / total).clamp(0.0, 1.0)
}
