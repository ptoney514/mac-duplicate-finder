//! Best-of view backing query (§9.5): one best image per gap-based event,
//! ranked by quality, then aesthetic, then sharpness.

mod common;

use common::{exif_jpeg, open_engine, scan, test_image, write_file, TempDir};

/// Two events: a morning trio (one deliberately blurred) and an afternoon
/// pair, plus an undated image that must not appear.
fn seed(lib: &std::path::Path) {
    let sharp = test_image(320, 200, true);
    let writes = [
        ("m1.jpg", sharp.clone(), "2023:09:10 09:00:00"),
        ("m2-blurry.jpg", sharp.blur(4.0), "2023:09:10 09:20:00"),
        ("m3.jpg", test_image(320, 200, false), "2023:09:10 09:40:00"),
        ("a1.jpg", test_image(300, 300, false), "2023:09:10 15:00:00"),
        (
            "a2-blurry.jpg",
            test_image(300, 300, true).blur(4.0),
            "2023:09:10 15:05:00",
        ),
    ];
    for (name, img, when) in writes {
        write_file(lib, name, &exif_jpeg(&img, "Apple", "iPhone 15", 1, when));
    }
    common::save_jpeg(&test_image(200, 100, true), lib, "undated.jpg");
}

#[test]
fn best_of_picks_one_sharp_winner_per_event() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    seed(&lib);
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);

    let entries = engine.best_of(2 * 60 * 60).unwrap();

    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(entries[0].start <= entries[1].start, "chronological");
    assert_eq!(entries[0].count, 3);
    assert_eq!(entries[1].count, 2);
    for entry in &entries {
        assert!(
            !entry.item.path.contains("blurry"),
            "blurred frames never win: {entry:?}"
        );
        assert!(entry.item.thumb_path.is_some());
        assert!(entry.start <= entry.end);
    }
}

#[test]
fn tighter_gaps_make_more_events() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    seed(&lib);
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);

    // 10 minutes: the morning trio (20-minute spacing) splits apart.
    let entries = engine.best_of(10 * 60).unwrap();
    assert_eq!(entries.len(), 4, "{entries:?}");
}
