//! Incremental rescan logic: a file whose path, size, and mtime are unchanged
//! is never re-read; changes, deletions, and reappearances are tracked.

mod common;

use common::{mtime_of, open_engine, scan, set_mtime, stored_hash, write_file, TempDir};

/// Payload over 4KB so tests can choose whether an edit lands inside or
/// outside the prehash window.
fn payload(fill: u8) -> Vec<u8> {
    vec![fill; 8192]
}

#[test]
fn rescan_with_no_changes_hashes_nothing() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    write_file(&lib, "a.jpg", &payload(1));
    write_file(&lib, "b.jpg", &payload(1));
    let mut engine = open_engine(dir.path());

    let first = scan(&mut engine, &lib);
    assert_eq!(first.found, 2);
    assert_eq!(first.added, 2);
    assert_eq!(first.hashed, 2, "same-size pair must be hashed");
    assert_eq!(first.analyzed, 2);

    let second = scan(&mut engine, &lib);
    assert_eq!(second.found, 2);
    assert_eq!(second.added, 0);
    assert_eq!(second.unchanged, 2);
    assert_eq!(second.hashed, 0, "unchanged files must not be re-hashed");
    assert_eq!(
        second.analyzed, 0,
        "unchanged files must not be re-analyzed"
    );
}

#[test]
fn unchanged_size_and_mtime_skips_rehash_even_if_content_differs() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let a = write_file(&lib, "a.jpg", &payload(1));
    write_file(&lib, "b.jpg", &payload(1));
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);
    let original_hash = stored_hash(dir.path(), &a).unwrap();
    let original_mtime = mtime_of(&a);

    // Same length, different bytes, mtime restored: invisible to the
    // path+size+mtime check by design.
    write_file(&lib, "a.jpg", &payload(9));
    set_mtime(&a, original_mtime);

    let summary = scan(&mut engine, &lib);
    assert_eq!(summary.unchanged, 2);
    assert_eq!(summary.hashed, 0);
    assert_eq!(stored_hash(dir.path(), &a).unwrap(), original_hash);
}

#[test]
fn modified_file_is_rehashed_and_leaves_its_dupe_group() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let a = write_file(&lib, "a.jpg", &payload(1));
    write_file(&lib, "b.jpg", &payload(1));
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);
    assert_eq!(engine.dupes().unwrap().len(), 1);

    // Same size and same first 4KB (so only the full hash can tell them
    // apart), newer mtime so the change is visible.
    let mut edited = payload(1);
    *edited.last_mut().unwrap() = 2;
    write_file(&lib, "a.jpg", &edited);
    set_mtime(&a, mtime_of(&a) + 10);

    let summary = scan(&mut engine, &lib);
    assert_eq!(summary.updated, 1);
    assert_eq!(summary.unchanged, 1);
    assert_eq!(summary.hashed, 1, "only the modified file needs a re-hash");
    assert!(engine.dupes().unwrap().is_empty());
}

#[test]
fn deleted_file_is_marked_missing_and_leaves_dupes() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    write_file(&lib, "a.jpg", &payload(1));
    write_file(&lib, "b.jpg", &payload(1));
    let c = write_file(&lib, "c.jpg", &payload(1));
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);
    assert_eq!(engine.dupes().unwrap()[0].files.len(), 3);

    std::fs::remove_file(&c).unwrap();

    let summary = scan(&mut engine, &lib);
    assert_eq!(summary.missing, 1);
    let dupes = engine.dupes().unwrap();
    assert_eq!(dupes.len(), 1);
    assert_eq!(dupes[0].files.len(), 2);
    assert!(dupes[0].files.iter().all(|f| !f.path.ends_with("c.jpg")));

    // Already-missing rows are not counted as newly missing again.
    let third = scan(&mut engine, &lib);
    assert_eq!(third.missing, 0);
}

#[test]
fn reappearing_unchanged_file_is_revived_without_rehash() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    write_file(&lib, "a.jpg", &payload(1));
    let b = write_file(&lib, "b.jpg", &payload(1));
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);
    let b_mtime = mtime_of(&b);

    std::fs::remove_file(&b).unwrap();
    scan(&mut engine, &lib);
    assert!(engine.dupes().unwrap().is_empty());

    write_file(&lib, "b.jpg", &payload(1));
    set_mtime(&b, b_mtime);

    let summary = scan(&mut engine, &lib);
    assert_eq!(summary.hashed, 0, "stored hash is still valid; no re-read");
    assert_eq!(summary.missing, 0);
    let dupes = engine.dupes().unwrap();
    assert_eq!(dupes.len(), 1);
    assert_eq!(dupes[0].files.len(), 2);
}

#[test]
fn candidate_stage_skips_unique_sizes_but_analysis_still_hashes() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let a = write_file(&lib, "a.jpg", &payload(1));
    let mut engine = open_engine(dir.path());

    let first = scan(&mut engine, &lib);
    assert_eq!(first.hashed, 0, "unique size can't be an exact dupe; skip");
    assert_eq!(first.analyzed, 1);
    assert!(
        stored_hash(dir.path(), &a).is_some(),
        "analysis hashes it anyway (thumbnail cache key)"
    );

    write_file(&lib, "twin.jpg", &payload(1));
    let second = scan(&mut engine, &lib);
    assert_eq!(second.added, 1);
    assert_eq!(second.hashed, 1, "only the twin still needs a hash");
    assert_eq!(engine.dupes().unwrap().len(), 1);
}
