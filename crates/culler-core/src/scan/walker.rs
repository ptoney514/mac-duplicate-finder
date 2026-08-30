//! Recursive image discovery with include/exclude rules.

use std::path::Path;
use std::time::UNIX_EPOCH;

use walkdir::{DirEntry, WalkDir};

use crate::{CoreError, FoundFile, Result};

/// File extensions treated as images (compared case-insensitively).
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "arw", "bmp", "cr2", "cr3", "dng", "gif", "heic", "heif", "jpeg", "jpg", "nef", "orf", "png",
    "raf", "rw2", "tif", "tiff", "webp",
];

/// Result of walking a root: the image files found plus a count of entries
/// skipped because of I/O errors.
#[derive(Debug, Default)]
pub struct WalkOutcome {
    pub files: Vec<FoundFile>,
    pub errors: u64,
}

/// Recursively finds image files under `root`. Hidden files and directories
/// (dot-prefixed) are skipped; symlinks are not followed. `on_found` receives
/// a running count for progress reporting. Errors on individual entries are
/// counted and skipped; an unreadable `root` is an error.
pub fn walk_images(root: &Path, on_found: &mut dyn FnMut(u64)) -> Result<WalkOutcome> {
    std::fs::metadata(root).map_err(|source| CoreError::Io {
        path: root.display().to_string(),
        source,
    })?;

    let mut out = WalkOutcome::default();
    let entries = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_hidden(e));
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                out.errors += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() || !has_image_extension(entry.path()) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                out.errors += 1;
                continue;
            }
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        out.files.push(FoundFile {
            path: entry.path().to_string_lossy().into_owned(),
            size: meta.len(),
            mtime,
        });
        if out.files.len() % 250 == 0 {
            on_found(out.files.len() as u64);
        }
    }
    on_found(out.files.len() as u64);
    Ok(out)
}

/// Dot-prefixed entries are excluded, but an explicitly chosen hidden root
/// (depth 0) still scans.
fn is_hidden(entry: &DirEntry) -> bool {
    entry.depth() > 0 && entry.file_name().to_string_lossy().starts_with('.')
}

fn has_image_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        IMAGE_EXTENSIONS
            .binary_search(&e.to_ascii_lowercase().as_str())
            .is_ok()
    })
}
