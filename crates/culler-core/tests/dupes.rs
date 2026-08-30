//! End-to-end exact-duplicate detection through the public API: walk, hash
//! with size-first grouping and prehash filtering, report groups.

mod common;

use common::{open_engine, scan, set_mtime, stored_hash, write_file, TempDir};

fn payload(fill: u8, len: usize) -> Vec<u8> {
    vec![fill; len]
}

#[test]
fn finds_exact_duplicates_and_sorts_by_reclaimable_bytes() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");

    // Big group: 3 identical 8KB copies → 16KB reclaimable.
    let original = write_file(&lib, "kids-beach.jpg", &payload(1, 8192));
    write_file(&lib, "copy of kids-beach.jpg", &payload(1, 8192));
    write_file(&lib, "nested/kids-beach (2).jpg", &payload(1, 8192));

    // Small group: 2 identical 1KB copies → 1KB reclaimable.
    write_file(&lib, "receipt.png", &payload(2, 1024));
    write_file(&lib, "receipt copy.png", &payload(2, 1024));

    // Same size as the big group but different first bytes: the prehash
    // rules it out of the candidate stage before a full read.
    let mut early_diff = payload(1, 8192);
    early_diff[10] = 99;
    let early = write_file(&lib, "early-diff.jpg", &early_diff);

    // Same size AND same first 4KB: only the full hash rules it out.
    let mut late_diff = payload(1, 8192);
    *late_diff.last_mut().unwrap() = 99;
    write_file(&lib, "late-diff.jpg", &late_diff);

    // Unique size: skipped by the candidate stage entirely.
    let unique = write_file(&lib, "unique.jpg", &payload(3, 500));

    // Keeper: oldest mtime wins.
    set_mtime(&original, 1_000_000_000);

    let mut engine = open_engine(dir.path());
    let summary = scan(&mut engine, &lib);
    // Candidate-stage hashing: 3 kids-beach + late-diff (prehash collision)
    // + 2 receipts = 6. early-diff and unique are left to the analysis pass.
    assert_eq!(summary.hashed, 6);
    let dupes = engine.dupes().unwrap();

    assert_eq!(dupes.len(), 2);

    let big = &dupes[0];
    assert_eq!(big.size, 8192);
    assert_eq!(big.files.len(), 3);
    assert_eq!(big.reclaimable, 2 * 8192);
    assert!(
        big.files[0].path.ends_with("/kids-beach.jpg"),
        "oldest mtime is the keeper: {big:?}"
    );

    let small = &dupes[1];
    assert_eq!(small.files.len(), 2);
    assert_eq!(small.reclaimable, 1024);

    // Analysis eventually hashes everything (thumbnail cache key), but the
    // near-misses still never join a group.
    assert!(stored_hash(dir.path(), &unique).is_some());
    assert!(stored_hash(dir.path(), &early).is_some());
    for group in &dupes {
        for f in &group.files {
            assert!(!f.path.contains("diff") && !f.path.contains("unique"));
        }
    }
}

#[test]
fn keeper_ties_break_on_shorter_path() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let a = write_file(&lib, "deeply/nested/photo.jpg", &payload(5, 4096));
    let b = write_file(&lib, "photo.jpg", &payload(5, 4096));
    set_mtime(&a, 1_000_000_000);
    set_mtime(&b, 1_000_000_000);

    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);
    let dupes = engine.dupes().unwrap();

    assert_eq!(dupes.len(), 1);
    assert!(dupes[0].files[0].path.ends_with("/lib/photo.jpg"));
}

#[test]
fn non_image_and_hidden_files_are_ignored() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    write_file(&lib, "notes.txt", &payload(7, 2048));
    write_file(&lib, "notes copy.txt", &payload(7, 2048));
    write_file(&lib, ".hidden.jpg", &payload(8, 2048));
    write_file(&lib, ".cache/thumb.jpg", &payload(8, 2048));

    let mut engine = open_engine(dir.path());
    let summary = scan(&mut engine, &lib);

    assert_eq!(summary.found, 0);
    assert_eq!(common::file_row_count(dir.path()), 0);
    assert!(engine.dupes().unwrap().is_empty());
}
