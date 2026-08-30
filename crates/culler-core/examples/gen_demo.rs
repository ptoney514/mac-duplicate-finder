//! Generates a small library of real JPEGs for exercising the CLI by hand:
//! exact duplicates, near duplicates (brightness/resize variants), and
//! distinct shots.
//!
//! Usage: cargo run --example gen_demo -- <output dir>

use image::{DynamicImage, RgbImage};

/// Synthetic "photo": gradient (or radial rings — a diagonal gradient would
/// share the horizontal one's dHash, since both are monotonic along x) plus
/// a soft disc.
fn shot(w: u32, h: u32, direction: u8) -> DynamicImage {
    let img = RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let t = match direction {
            0 => fx,
            1 => fy,
            _ => (((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt() * 2.0).min(1.0),
        };
        let v = (t * 255.0) as u8;
        let (dx, dy) = (fx - 0.5, fy - 0.5);
        let disc = if (dx * dx + dy * dy).sqrt() < 0.25 {
            60u8
        } else {
            0
        };
        image::Rgb([v.saturating_add(disc), v, 255 - v])
    });
    DynamicImage::ImageRgb8(img)
}

fn save(img: &DynamicImage, dir: &std::path::Path, name: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    img.save(&path).unwrap();
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .expect("usage: gen_demo <output dir>");

    let beach = shot(640, 400, 0);
    let city = shot(640, 400, 1);
    let sunset = shot(500, 500, 2);

    save(&beach, &dir, "vacation/beach.jpg");
    save(&beach.brighten(14), &dir, "vacation/beach-edited.jpg"); // near dupe
    save(
        &beach.resize(320, 200, image::imageops::FilterType::Triangle),
        &dir,
        "vacation/beach-small.jpg", // near dupe (downscaled export)
    );
    save(&city, &dir, "city.jpg");
    save(&sunset, &dir, "sunset.jpg");

    // Exact duplicates: byte-for-byte copies.
    std::fs::create_dir_all(dir.join("backup/vacation")).unwrap();
    std::fs::copy(
        dir.join("vacation/beach.jpg"),
        dir.join("backup/vacation/beach.jpg"),
    )
    .unwrap();
    std::fs::copy(dir.join("sunset.jpg"), dir.join("backup/sunset.jpg")).unwrap();

    println!("demo library written to {}", dir.display());
}
