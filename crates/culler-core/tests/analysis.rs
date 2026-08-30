//! The analysis pass through the public API: dimensions, EXIF, cached
//! thumbnails keyed by content hash, incremental behavior.

mod common;

use common::{
    count, exif_jpeg, images_row, open_engine, save_jpeg, scan, stored_hash, test_image,
    write_file, TempDir,
};

#[test]
fn analysis_records_dimensions_and_perceptual_hashes() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let photo = save_jpeg(&test_image(320, 200, true), &lib, "photo.jpg");
    let mut engine = open_engine(dir.path());

    let summary = scan(&mut engine, &lib);

    assert_eq!(summary.analyzed, 1);
    assert_eq!(summary.errors, 0);
    let row = images_row(dir.path(), &photo).expect("images row written");
    assert_eq!(row.width, Some(320));
    assert_eq!(row.height, Some(200));
    assert!(row.dhash.is_some() && row.phash.is_some());
    assert!(
        stored_hash(dir.path(), &photo).is_some(),
        "analysis must content-hash every file (thumb cache key)"
    );
}

#[test]
fn analysis_extracts_exif_facts() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let bytes = exif_jpeg(
        &test_image(200, 150, true),
        "Apple",
        "iPhone 15 Pro",
        6,
        "2019:07:15 14:30:05",
    );
    let photo = write_file(&lib, "exif.jpg", &bytes);
    let mut engine = open_engine(dir.path());

    scan(&mut engine, &lib);

    let row = images_row(dir.path(), &photo).expect("images row written");
    assert_eq!(row.captured_at, Some(1563201005));
    assert_eq!(row.camera.as_deref(), Some("Apple iPhone 15 Pro"));
    assert_eq!(row.orientation, Some(6));
}

#[test]
fn thumbnails_are_256_long_edge_and_shared_by_content() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let img = test_image(1024, 768, true);
    let a = save_jpeg(&img, &lib, "a.jpg");
    let bytes = std::fs::read(&a).unwrap();
    let b = write_file(&lib, "b.jpg", &bytes); // identical content
    let mut engine = open_engine(dir.path());

    scan(&mut engine, &lib);

    let row_a = images_row(dir.path(), &a).unwrap();
    let row_b = images_row(dir.path(), &b).unwrap();
    let thumb = row_a.thumb_path.clone().expect("thumbnail generated");
    assert_eq!(row_a.thumb_path, row_b.thumb_path, "keyed by content hash");
    let hash_hex: String = stored_hash(dir.path(), &a)
        .unwrap()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert!(
        thumb.contains(&hash_hex),
        "thumb path {thumb} not keyed by {hash_hex}"
    );
    let decoded = image::open(&thumb).expect("thumbnail decodes");
    assert_eq!((decoded.width(), decoded.height()), (256, 192));
}

#[test]
fn small_images_are_not_upscaled() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let tiny = save_jpeg(&test_image(100, 80, true), &lib, "tiny.jpg");
    let mut engine = open_engine(dir.path());

    scan(&mut engine, &lib);

    let thumb = images_row(dir.path(), &tiny).unwrap().thumb_path.unwrap();
    let decoded = image::open(&thumb).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (100, 80));
}

#[test]
fn undecodable_file_still_gets_an_analyzed_row() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let junk = write_file(&lib, "corrupt.jpg", &[0xABu8; 5000].repeat(3));
    let mut engine = open_engine(dir.path());

    let summary = scan(&mut engine, &lib);

    assert_eq!(
        summary.analyzed, 1,
        "row recorded so it isn't retried forever"
    );
    assert!(
        summary.errors >= 1,
        "decode failure surfaces as an error count"
    );
    let row = images_row(dir.path(), &junk).expect("images row written");
    assert_eq!(row.width, None);
    assert_eq!(row.dhash, None);
    assert_eq!(row.thumb_path, None);
    assert_eq!(
        count(
            dir.path(),
            "SELECT COUNT(*) FROM files WHERE status = 'analyzed'"
        ),
        1
    );
}

#[test]
fn analysis_is_incremental_and_rescans_changed_files() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let a = save_jpeg(&test_image(320, 200, true), &lib, "a.jpg");
    save_jpeg(&test_image(300, 300, false), &lib, "b.jpg");
    let mut engine = open_engine(dir.path());

    let first = scan(&mut engine, &lib);
    assert_eq!(first.analyzed, 2);

    let second = scan(&mut engine, &lib);
    assert_eq!(second.analyzed, 0, "unchanged files are never re-analyzed");

    // Replace a.jpg with a different real image (different size -> changed).
    save_jpeg(&test_image(400, 240, false), &lib, "a.jpg");
    let third = scan(&mut engine, &lib);
    assert_eq!(third.updated, 1);
    assert_eq!(third.analyzed, 1, "only the changed file is re-analyzed");
    let row = images_row(dir.path(), &a).unwrap();
    assert_eq!(row.width, Some(400), "stale analysis was replaced");
}
