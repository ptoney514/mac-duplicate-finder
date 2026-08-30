//! Queries backing the Swift commit flow (§9.7): resolving content hashes
//! back to the live file copies that would be staged.

mod common;

use common::{open_engine, scan, solid_jpeg, write_file, TempDir};

#[test]
fn files_for_hashes_returns_live_copies_with_sizes() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let original = solid_jpeg(&lib, "cherry.jpg", [220, 20, 20], 64);
    let bytes = std::fs::read(&original).unwrap();
    let copy = write_file(&lib, "cherry copy.jpg", &bytes);
    solid_jpeg(&lib, "ocean.jpg", [20, 20, 220], 64);
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);

    let hash_hex = engine.dupes().unwrap()[0]
        .hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    let groups = engine
        .files_for_hashes(std::slice::from_ref(&hash_hex))
        .unwrap();
    assert_eq!(groups.len(), 1);
    let (hex, files) = &groups[0];
    assert_eq!(hex, &hash_hex);
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|(_, size)| *size == bytes.len() as u64));

    // A staged/deleted copy drops out after a rescan.
    std::fs::remove_file(&copy).unwrap();
    scan(&mut engine, &lib);
    let groups = engine.files_for_hashes(&[hash_hex]).unwrap();
    assert_eq!(groups[0].1.len(), 1);

    // Unknown hashes come back empty rather than erroring.
    let unknown = engine.files_for_hashes(&["ab".repeat(32)]).unwrap();
    assert!(unknown[0].1.is_empty());
}
