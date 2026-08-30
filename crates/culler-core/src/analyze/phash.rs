//! 64-bit perceptual hashes: dHash (gradient) and pHash (DCT), PRD 5.1.

use image::imageops::FilterType;
use image::DynamicImage;

/// Difference hash: 9x8 grayscale downscale, one bit per horizontal
/// brightness increase. Robust to scaling and small tonal shifts.
pub fn dhash(img: &DynamicImage) -> u64 {
    let g = img.resize_exact(9, 8, FilterType::Triangle).to_luma8();
    let mut bits = 0u64;
    let mut bit = 0;
    for y in 0..8 {
        for x in 0..8 {
            if g.get_pixel(x, y)[0] > g.get_pixel(x + 1, y)[0] {
                bits |= 1 << bit;
            }
            bit += 1;
        }
    }
    bits
}

/// DCT hash: 32x32 grayscale downscale, 2D DCT-II, one bit per low-frequency
/// coefficient above the median of the top-left 8x8 block.
pub fn phash(img: &DynamicImage) -> u64 {
    const N: usize = 32;
    let g = img
        .resize_exact(N as u32, N as u32, FilterType::Triangle)
        .to_luma8();
    let mut pixels = [[0f64; N]; N];
    for (y, row) in pixels.iter_mut().enumerate() {
        for (x, v) in row.iter_mut().enumerate() {
            *v = g.get_pixel(x as u32, y as u32)[0] as f64;
        }
    }

    let dct1d = |v: &[f64; N]| -> [f64; N] {
        std::array::from_fn(|k| {
            v.iter()
                .enumerate()
                .map(|(n, x)| {
                    x * ((std::f64::consts::PI / N as f64) * (n as f64 + 0.5) * k as f64).cos()
                })
                .sum()
        })
    };
    let mut rows = [[0f64; N]; N];
    for y in 0..N {
        rows[y] = dct1d(&pixels[y]);
    }
    let mut dct = [[0f64; N]; N];
    for x in 0..N {
        let column: [f64; N] = std::array::from_fn(|y| rows[y][x]);
        let transformed = dct1d(&column);
        for y in 0..N {
            dct[y][x] = transformed[y];
        }
    }

    let mut block = [0f64; 64];
    for y in 0..8 {
        for x in 0..8 {
            block[y * 8 + x] = dct[y][x];
        }
    }
    let mut sorted = block;
    sorted.sort_by(f64::total_cmp);
    let median = (sorted[31] + sorted[32]) / 2.0;
    block.iter().enumerate().fold(
        0u64,
        |bits, (i, v)| if *v > median { bits | 1 << i } else { bits },
    )
}

/// Hamming distance between two 64-bit hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}
