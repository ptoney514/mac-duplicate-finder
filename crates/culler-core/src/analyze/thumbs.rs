//! Thumbnail cache: 256px long edge, JPEG (ADR-0001), keyed by content hash
//! so identical files share one thumbnail and moves never invalidate it.

use std::io;
use std::path::{Path, PathBuf};

use image::DynamicImage;

/// Long edge of generated thumbnails, in pixels.
pub const THUMB_LONG_EDGE: u32 = 256;

/// Returns the cache path for `content_hash` under `thumbs_dir`, generating
/// the thumbnail from `img` if it isn't cached yet. Images already at or
/// under the target size are stored as-is (never upscaled). Writes go through
/// a temp file + rename so concurrent analyzers can't corrupt the cache.
pub fn ensure_thumb(
    img: &DynamicImage,
    content_hash: &[u8; 32],
    thumbs_dir: &Path,
) -> io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};

    let hex: String = content_hash.iter().map(|b| format!("{b:02x}")).collect();
    let subdir = thumbs_dir.join(&hex[..2]);
    let path = subdir.join(format!("{hex}.jpg"));
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(&subdir)?;

    let thumb = if img.width().max(img.height()) > THUMB_LONG_EDGE {
        img.resize(
            THUMB_LONG_EDGE,
            THUMB_LONG_EDGE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img.clone()
    };

    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let tmp = subdir.join(format!(
        "{hex}.{}.{}.tmp",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let write = || -> io::Result<()> {
        let mut writer = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 80);
        thumb
            .to_rgb8()
            .write_with_encoder(encoder)
            .map_err(io::Error::other)?;
        use std::io::Write;
        writer.flush()?;
        Ok(())
    };
    match write().and_then(|()| std::fs::rename(&tmp, &path)) {
        Ok(()) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}
