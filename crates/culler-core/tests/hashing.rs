//! Unit tests for BLAKE3 content hashing and the first-4KB prehash.

mod common;

use common::{write_file, TempDir};
use culler_core::scan::hash::{hash_file, prehash_file, PREHASH_BYTES};

#[test]
fn hash_matches_blake3_of_contents() {
    let dir = TempDir::new();
    let contents = b"culler milestone one".repeat(1000);
    let path = write_file(dir.path(), "a.jpg", &contents);

    let got = hash_file(&path).unwrap();

    assert_eq!(got, *blake3::hash(&contents).as_bytes());
}

#[test]
fn identical_contents_hash_identically() {
    let dir = TempDir::new();
    let contents = vec![0x7Au8; 10_000];
    let a = write_file(dir.path(), "a.jpg", &contents);
    let b = write_file(dir.path(), "b.jpg", &contents);

    assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
}

#[test]
fn different_contents_hash_differently() {
    let dir = TempDir::new();
    let a = write_file(dir.path(), "a.jpg", &vec![1u8; 5000]);
    let b = write_file(dir.path(), "b.jpg", &vec![2u8; 5000]);

    assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
}

#[test]
fn prehash_covers_only_the_first_4kb() {
    let dir = TempDir::new();
    let mut one = vec![0xAAu8; 8192];
    let mut two = one.clone();
    two[5000] = 0xBB; // differs only after the prehash window
    let a = write_file(dir.path(), "a.jpg", &one);
    let b = write_file(dir.path(), "b.jpg", &two);

    assert_eq!(prehash_file(&a).unwrap(), prehash_file(&b).unwrap());
    assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());

    // And a difference inside the window changes the prehash.
    one[100] = 0xCC;
    let c = write_file(dir.path(), "c.jpg", &one);
    assert_ne!(prehash_file(&a).unwrap(), prehash_file(&c).unwrap());
}

#[test]
fn prehash_of_short_file_equals_full_hash() {
    let dir = TempDir::new();
    let contents = b"tiny".to_vec();
    assert!((contents.len() as u64) < PREHASH_BYTES);
    let path = write_file(dir.path(), "tiny.jpg", &contents);

    assert_eq!(prehash_file(&path).unwrap(), hash_file(&path).unwrap());
    assert_eq!(
        prehash_file(&path).unwrap(),
        *blake3::hash(&contents).as_bytes()
    );
}
