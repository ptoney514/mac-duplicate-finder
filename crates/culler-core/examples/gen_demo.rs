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

/// JPEG with a minimal EXIF APP1 (camera + DateTimeOriginal) so burst
/// clustering has capture times to work with.
fn save_with_exif(img: &DynamicImage, dir: &std::path::Path, name: &str, datetime: &str) {
    let mut jpeg = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut jpeg),
        image::ImageFormat::Jpeg,
    )
    .unwrap();

    let (make, model) = (b"Apple\0".as_slice(), b"iPhone 15 Pro\0".as_slice());
    let mut dto = datetime.as_bytes().to_vec();
    dto.push(0);

    let mut tiff = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
    let entry = |out: &mut Vec<u8>, tag: u16, typ: u16, cnt: u32, val: u32| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&typ.to_le_bytes());
        out.extend_from_slice(&cnt.to_le_bytes());
        out.extend_from_slice(&val.to_le_bytes());
    };
    // IFD0: Make, Model, ExifIFD pointer -> 2 + 3*12 + 4 = 42; data at 50.
    let make_off = 50u32;
    let model_off = make_off + make.len() as u32;
    let exif_ifd_off = model_off + model.len() as u32;
    let dto_off = exif_ifd_off + 18;
    tiff.extend_from_slice(&3u16.to_le_bytes());
    entry(&mut tiff, 0x010F, 2, make.len() as u32, make_off);
    entry(&mut tiff, 0x0110, 2, model.len() as u32, model_off);
    entry(&mut tiff, 0x8769, 4, 1, exif_ifd_off);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(make);
    tiff.extend_from_slice(model);
    tiff.extend_from_slice(&1u16.to_le_bytes());
    entry(&mut tiff, 0x9003, 2, dto.len() as u32, dto_off);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(&dto);

    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    out.extend_from_slice(&jpeg[2..]);

    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, out).unwrap();
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

    // A burst: four near-identical frames one second apart, one slightly
    // blurred (should lose the keeper race), plus an unrelated shot minutes
    // later on the same camera.
    let base = shot(640, 400, 2);
    save_with_exif(&base, &dir, "burst/frame-1.jpg", "2024:05:12 14:03:01");
    save_with_exif(
        &base.brighten(6),
        &dir,
        "burst/frame-2.jpg",
        "2024:05:12 14:03:02",
    );
    save_with_exif(
        &base.blur(3.0),
        &dir,
        "burst/frame-3-blurry.jpg",
        "2024:05:12 14:03:03",
    );
    save_with_exif(
        &base.brighten(-6),
        &dir,
        "burst/frame-4.jpg",
        "2024:05:12 14:03:04",
    );
    save_with_exif(
        &shot(640, 400, 1),
        &dir,
        "burst/later.jpg",
        "2024:05:12 14:20:00",
    );

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
