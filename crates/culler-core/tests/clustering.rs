//! Near clustering end to end: analyze real images, cluster by perceptual
//! hashes, re-run with different thresholds without re-analyzing.

mod common;

use common::{count, open_engine, save_jpeg, scan, test_image, write_file, TempDir};
use culler_core::cluster::near::{DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX};

#[test]
fn near_duplicates_cluster_end_to_end() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let base = test_image(320, 200, true);
    save_jpeg(&base, &lib, "base.jpg");
    save_jpeg(&base.brighten(14), &lib, "bright.jpg");
    save_jpeg(&test_image(320, 200, false), &lib, "distinct.jpg");
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);

    let clusters = engine
        .cluster_near(DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX)
        .unwrap();

    assert_eq!(clusters.len(), 1, "{clusters:?}");
    let files = &clusters[0].files;
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|f| f.ends_with("base.jpg")));
    assert!(files.iter().any(|f| f.ends_with("bright.jpg")));

    // Persisted per PRD section 6.
    assert_eq!(
        count(
            dir.path(),
            "SELECT COUNT(*) FROM clusters WHERE kind = 'near'"
        ),
        1
    );
    assert_eq!(count(dir.path(), "SELECT COUNT(*) FROM cluster_members"), 2);
}

#[test]
fn reclustering_replaces_rows_instead_of_accumulating() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let base = test_image(320, 200, true);
    save_jpeg(&base, &lib, "base.jpg");
    save_jpeg(&base.brighten(14), &lib, "bright.jpg");
    save_jpeg(&test_image(320, 200, false), &lib, "distinct.jpg");
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);

    // Absurdly loose thresholds: everything lands in one cluster.
    let loose = engine.cluster_near(64, 64).unwrap();
    assert_eq!(loose.len(), 1);
    assert_eq!(loose[0].files.len(), 3);

    // Back to defaults without re-analyzing: rows replaced, not appended.
    let normal = engine
        .cluster_near(DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX)
        .unwrap();
    assert_eq!(normal.len(), 1);
    assert_eq!(normal[0].files.len(), 2);
    assert_eq!(
        count(
            dir.path(),
            "SELECT COUNT(*) FROM clusters WHERE kind = 'near'"
        ),
        1
    );
    assert_eq!(count(dir.path(), "SELECT COUNT(*) FROM cluster_members"), 2);
}

#[test]
fn exact_duplicates_collapse_to_one_representative_in_near_clusters() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    let base = test_image(320, 200, true);
    let original = save_jpeg(&base, &lib, "original.jpg");
    let bytes = std::fs::read(&original).unwrap();
    write_file(&lib, "byte-copy.jpg", &bytes); // exact dupe: dupes flow's job
    save_jpeg(&base.brighten(14), &lib, "bright.jpg");
    let mut engine = open_engine(dir.path());
    scan(&mut engine, &lib);

    let clusters = engine
        .cluster_near(DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX)
        .unwrap();

    assert_eq!(clusters.len(), 1, "{clusters:?}");
    assert_eq!(
        clusters[0].files.len(),
        2,
        "identical content collapses to one member: {clusters:?}"
    );
    assert!(
        engine.dupes().unwrap().len() == 1,
        "exact pair still in dupes"
    );
}
