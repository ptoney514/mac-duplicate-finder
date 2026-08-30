//! BLAKE3 content hashing. Exact-duplicate detection only ever full-hashes
//! files that share a size with another file, and among those, only files
//! whose first-4KB prehash also collides.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Bytes covered by the prehash.
pub const PREHASH_BYTES: u64 = 4096;

/// BLAKE3 of the first [`PREHASH_BYTES`] of the file (the whole file if
/// shorter). Cheap collision filter run before full hashing.
pub fn prehash_file(path: &Path) -> io::Result<[u8; 32]> {
    let file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(file.take(PREHASH_BYTES))?;
    Ok(*hasher.finalize().as_bytes())
}

/// BLAKE3 of the entire file contents, streamed.
pub fn hash_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(path)?)?;
    Ok(*hasher.finalize().as_bytes())
}
