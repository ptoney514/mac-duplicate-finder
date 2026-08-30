//! The grid query backing the app's library view: analyzed images, newest
//! capture first, paginated.

mod common;

use common::{exif_jpeg, open_engine, save_jpeg, scan, test_image, write_file, TempDir};

#[test]
fn grid_lists_analyzed_images_newest_first_with_thumbs() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    write_file(
        &lib,
        "old.jpg",
        &exif_jpeg(
            &test_image(320, 200, true),
            "Apple",
            "iPhone 11",
            1,
            "2019:03:01 10:00:00",
        ),
    );
    write_file(
        &lib,
        "new.jpg",
        &exif_jpeg(
            &test_image(300, 300, false),
            "Apple",
            "iPhone 15",
            1,
            "2021:08:20 18:15:00",
        ),
    );
    save_jpeg(&test_image(200, 100, true), &lib, "undated.jpg");
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);

    let items = engine.grid_items(0, 10).unwrap();

    let names: Vec<&str> = items
        .iter()
        .map(|i| i.path.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        names,
        ["new.jpg", "old.jpg", "undated.jpg"],
        "newest capture first, undated last"
    );
    for item in &items {
        assert!(item.thumb_path.is_some(), "{item:?}");
        assert!(item.width.is_some() && item.height.is_some());
    }
    assert_eq!(items[0].width, Some(300));

    // Pagination.
    let page = engine.grid_items(1, 1).unwrap();
    assert_eq!(page.len(), 1);
    assert!(page[0].path.ends_with("old.jpg"));
    assert!(engine.grid_items(3, 10).unwrap().is_empty());
}

#[test]
fn grid_excludes_missing_files() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    save_jpeg(&test_image(320, 200, true), &lib, "keep.jpg");
    let gone = save_jpeg(&test_image(300, 300, false), &lib, "gone.jpg");
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);
    assert_eq!(engine.grid_items(0, 10).unwrap().len(), 2);

    std::fs::remove_file(&gone).unwrap();
    scan(&mut engine, &lib);

    let items = engine.grid_items(0, 10).unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].path.ends_with("keep.jpg"));
}
