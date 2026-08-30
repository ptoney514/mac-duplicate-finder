//! Shared test helpers. `TempDir` is a std-only stand-in for the usual
//! tempfile crate, kept out per the PRD's dependency rule (section 15).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use culler_core::{Engine, ScanSummary};
use image::{DynamicImage, RgbImage};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Unique directory removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("culler-test-{}-{n}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes `contents` to `dir/name` (creating parent dirs) and returns the path.
pub fn write_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, contents).unwrap();
    path
}

pub fn set_mtime(path: &Path, unix_secs: i64) {
    let t = UNIX_EPOCH + Duration::from_secs(u64::try_from(unix_secs).unwrap());
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(t).unwrap();
}

pub fn mtime_of(path: &Path) -> i64 {
    let m = std::fs::metadata(path).unwrap().modified().unwrap();
    i64::try_from(m.duration_since(UNIX_EPOCH).unwrap().as_secs()).unwrap()
}

/// Scans with a no-op progress callback.
pub fn scan(engine: &mut Engine, root: &Path) -> ScanSummary {
    engine.scan(root, &mut |_| {}).unwrap()
}

/// Opens an engine on a db file inside `dir`.
pub fn open_engine(dir: &Path) -> Engine {
    Engine::open(&dir.join("culler.db")).unwrap()
}

/// Reads `files.content_hash` for a path through a second connection.
/// Returns None if the row has no hash. Panics if the row is absent.
pub fn stored_hash(dir: &Path, path: &Path) -> Option<Vec<u8>> {
    let conn = rusqlite::Connection::open(dir.join("culler.db")).unwrap();
    conn.query_row(
        "SELECT content_hash FROM files WHERE path = ?1",
        [path.to_str().unwrap()],
        |row| row.get(0),
    )
    .unwrap()
}

/// Number of rows in `files`.
pub fn file_row_count(dir: &Path) -> i64 {
    let conn = rusqlite::Connection::open(dir.join("culler.db")).unwrap();
    conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .unwrap()
}

/// Runs an arbitrary COUNT-style query against the engine db.
pub fn count(dir: &Path, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open(dir.join("culler.db")).unwrap();
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

/// One `images` row, joined by file path.
#[derive(Debug)]
pub struct ImagesRow {
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub captured_at: Option<i64>,
    pub camera: Option<String>,
    pub orientation: Option<i64>,
    pub dhash: Option<i64>,
    pub phash: Option<i64>,
    pub thumb_path: Option<String>,
}

pub fn images_row(dir: &Path, path: &Path) -> Option<ImagesRow> {
    let conn = rusqlite::Connection::open(dir.join("culler.db")).unwrap();
    conn.query_row(
        "SELECT i.width, i.height, i.captured_at, i.camera, i.orientation, \
                i.dhash, i.phash, i.thumb_path \
         FROM images i JOIN files f ON f.id = i.file_id WHERE f.path = ?1",
        [path.to_str().unwrap()],
        |row| {
            Ok(ImagesRow {
                width: row.get(0)?,
                height: row.get(1)?,
                captured_at: row.get(2)?,
                camera: row.get(3)?,
                orientation: row.get(4)?,
                dhash: row.get(5)?,
                phash: row.get(6)?,
                thumb_path: row.get(7)?,
            })
        },
    )
    .ok()
}

/// Test stand-in for the CLIP embedder: maps an image to its average color
/// on the first three axes, and the words "red"/"green"/"blue" to the
/// matching axis. Lets search tests run without ONNX models.
pub struct StubEmbedder;

impl culler_core::embed::Embedder for StubEmbedder {
    fn embed_image(&self, img: &image::DynamicImage) -> culler_core::Result<Vec<f32>> {
        let rgb = img.to_rgb8();
        let n = (rgb.width() * rgb.height()) as f32;
        let mut v = vec![0.0f32; culler_core::embed::EMBED_DIM];
        for p in rgb.pixels() {
            v[0] += p[0] as f32 / 255.0 / n;
            v[1] += p[1] as f32 / 255.0 / n;
            v[2] += p[2] as f32 / 255.0 / n;
        }
        culler_core::embed::normalize(&mut v);
        Ok(v)
    }

    fn embed_text(&self, text: &str) -> culler_core::Result<Vec<f32>> {
        let mut v = vec![0.0f32; culler_core::embed::EMBED_DIM];
        match text {
            "red" => v[0] = 1.0,
            "green" => v[1] = 1.0,
            "blue" => v[2] = 1.0,
            _ => v[3] = 1.0,
        }
        Ok(v)
    }
}

/// Solid-color image in memory.
pub fn test_solid(r: u8, g: u8, b: u8) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, image::Rgb([r, g, b])))
}

/// Solid-color JPEG (survives thumbnailing unchanged in hue).
pub fn solid_jpeg(dir: &Path, name: &str, rgb: [u8; 3], side: u32) -> PathBuf {
    let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(side, side, image::Rgb(rgb)));
    save_jpeg(&img, dir, name)
}

/// Structured synthetic photo: gradient plus a soft disc, so perceptual
/// hashes are stable and orientation of the gradient distinguishes images.
pub fn test_image(w: u32, h: u32, horizontal: bool) -> DynamicImage {
    let img = RgbImage::from_fn(w, h, |x, y| {
        let t = if horizontal {
            x as f32 / w as f32
        } else {
            y as f32 / h as f32
        };
        let v = (t * 255.0) as u8;
        let dx = x as f32 / w as f32 - 0.5;
        let dy = y as f32 / h as f32 - 0.5;
        let disc = if (dx * dx + dy * dy).sqrt() < 0.25 {
            60u8
        } else {
            0
        };
        image::Rgb([v.saturating_add(disc), v, 255 - v])
    });
    DynamicImage::ImageRgb8(img)
}

pub fn jpeg_bytes(img: &DynamicImage) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    buf
}

pub fn save_jpeg(img: &DynamicImage, dir: &Path, name: &str) -> PathBuf {
    write_file(dir, name, &jpeg_bytes(img))
}

/// Encodes `img` as JPEG and splices in a hand-built EXIF APP1 segment
/// (kamadak-exif is read-only, so tests construct the TIFF bytes directly).
pub fn exif_jpeg(
    img: &DynamicImage,
    make: &str,
    model: &str,
    orientation: u16,
    datetime: &str,
) -> Vec<u8> {
    let jpeg = jpeg_bytes(img);
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "expected SOI");

    let mut make_z = make.as_bytes().to_vec();
    make_z.push(0);
    let mut model_z = model.as_bytes().to_vec();
    model_z.push(0);
    let mut dto_z = datetime.as_bytes().to_vec();
    dto_z.push(0);

    // TIFF header, little endian, IFD0 at offset 8.
    let mut tiff = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];

    // IFD0: Make, Model, Orientation, ExifIFD pointer. 2 + 4*12 + 4 = 54
    // bytes, so its data area starts at offset 62.
    let make_off = 62u32;
    let model_off = make_off + make_z.len() as u32;
    let mut exif_ifd_off = model_off + model_z.len() as u32;
    if exif_ifd_off % 2 == 1 {
        exif_ifd_off += 1; // keep IFD word-aligned
    }
    // Exif IFD: one entry (DateTimeOriginal): 2 + 12 + 4 = 18 bytes.
    let dto_off = exif_ifd_off + 18;

    let entry = |out: &mut Vec<u8>, tag: u16, typ: u16, cnt: u32, val: u32| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&typ.to_le_bytes());
        out.extend_from_slice(&cnt.to_le_bytes());
        out.extend_from_slice(&val.to_le_bytes());
    };

    tiff.extend_from_slice(&4u16.to_le_bytes());
    entry(&mut tiff, 0x010F, 2, make_z.len() as u32, make_off); // Make, ASCII
    entry(&mut tiff, 0x0110, 2, model_z.len() as u32, model_off); // Model
    entry(&mut tiff, 0x0112, 3, 1, orientation as u32); // Orientation, SHORT
    entry(&mut tiff, 0x8769, 4, 1, exif_ifd_off); // Exif IFD pointer, LONG
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD

    tiff.extend_from_slice(&make_z);
    tiff.extend_from_slice(&model_z);
    while tiff.len() < exif_ifd_off as usize {
        tiff.push(0);
    }
    tiff.extend_from_slice(&1u16.to_le_bytes());
    entry(&mut tiff, 0x9003, 2, dto_z.len() as u32, dto_off); // DateTimeOriginal
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(&dto_z);

    let mut app1 = vec![0xFF, 0xE1];
    let len = (2 + 6 + tiff.len()) as u16;
    app1.extend_from_slice(&len.to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);

    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}
