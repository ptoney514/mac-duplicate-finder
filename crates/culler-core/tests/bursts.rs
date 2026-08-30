//! Burst clustering end to end (stub embedder for cosine similarity),
//! keeper proposals, cluster listing, and face-fact storage.

mod common;

use common::{
    count, exif_jpeg, images_row, open_engine, scan, test_solid, write_file, StubEmbedder, TempDir,
};

/// Three near-identical "burst" shots one second apart plus one unrelated
/// shot hours later, all on the same camera.
fn seed_burst_library(lib: &std::path::Path) {
    let shots = [
        ("burst-1.jpg", [220u8, 20, 20], "2021:06:01 10:00:00"),
        ("burst-2.jpg", [219, 21, 20], "2021:06:01 10:00:01"),
        ("burst-3.jpg", [221, 20, 21], "2021:06:01 10:00:02"),
        ("sunset.jpg", [20, 20, 220], "2021:06:01 18:00:00"),
    ];
    for (name, rgb, when) in shots {
        let bytes = exif_jpeg(
            &test_solid(rgb[0], rgb[1], rgb[2]),
            "Apple",
            "iPhone 15",
            1,
            when,
        );
        write_file(lib, name, &bytes);
    }
}

#[test]
fn bursts_cluster_with_keepers_end_to_end() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    seed_burst_library(&lib);
    let mut engine = open_engine(dir.path());
    engine.attach_embedder(Box::new(StubEmbedder));
    scan(&mut engine, &lib);

    let bursts = engine.cluster_bursts(3, 0.92).unwrap();

    assert_eq!(bursts.len(), 1, "{bursts:?}");
    assert_eq!(bursts[0].files.len(), 3);
    assert!(bursts[0].files.iter().all(|f| f.contains("burst-")));

    // Persisted with a keeper and per-member quality scores.
    assert_eq!(
        count(
            dir.path(),
            "SELECT COUNT(*) FROM clusters WHERE kind = 'burst'"
        ),
        1
    );
    assert_eq!(
        count(
            dir.path(),
            "SELECT COUNT(*) FROM clusters WHERE kind = 'burst' AND keeper_file_id IS NOT NULL"
        ),
        1
    );
    let row = images_row(dir.path(), &lib.join("burst-1.jpg")).unwrap();
    let quality = row.quality_score.expect("quality written for members");
    assert!((0.0..=1.0).contains(&quality));

    // Re-running replaces rows instead of accumulating.
    engine.cluster_bursts(3, 0.92).unwrap();
    assert_eq!(
        count(
            dir.path(),
            "SELECT COUNT(*) FROM clusters WHERE kind = 'burst'"
        ),
        1
    );
}

#[test]
fn cluster_listing_returns_members_with_metadata() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    seed_burst_library(&lib);
    let mut engine = open_engine(dir.path());
    engine.attach_embedder(Box::new(StubEmbedder));
    scan(&mut engine, &lib);
    engine.cluster_bursts(3, 0.92).unwrap();

    let clusters = engine.clusters(Some("burst")).unwrap();

    assert_eq!(clusters.len(), 1);
    let cluster = &clusters[0];
    assert_eq!(cluster.kind, "burst");
    assert_eq!(cluster.members.len(), 3);
    assert!(cluster.keeper_file_id.is_some());
    assert!(cluster
        .members
        .iter()
        .any(|m| Some(m.file_id) == cluster.keeper_file_id));
    // Filmstrip order: capture time ascending.
    let times: Vec<_> = cluster.members.iter().map(|m| m.captured_at).collect();
    let mut sorted = times.clone();
    sorted.sort();
    assert_eq!(times, sorted);
    for member in &cluster.members {
        assert!(member.thumb_path.is_some());
        assert!(member.quality_score.is_some());
        assert_eq!(member.content_hash_hex.len(), 64);
    }

    assert!(engine.clusters(Some("near")).unwrap().is_empty());
    assert_eq!(engine.clusters(None).unwrap().len(), 1);
}

#[test]
fn face_facts_store_and_influence_keepers() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    seed_burst_library(&lib);
    let mut engine = open_engine(dir.path());
    engine.attach_embedder(Box::new(StubEmbedder));
    scan(&mut engine, &lib);
    engine.cluster_bursts(3, 0.92).unwrap();

    let listed = engine.clusters(Some("burst")).unwrap();
    let ids: Vec<i64> = listed[0].members.iter().map(|m| m.file_id).collect();
    let old_keeper = listed[0].keeper_file_id.unwrap();
    // The current keeper has closed eyes; everyone else has them open.
    let facts: Vec<(i64, u32, f64)> = ids
        .iter()
        .map(|&id| (id, 2, if id == old_keeper { 0.0 } else { 1.0 }))
        .collect();
    engine.store_face_facts(&facts).unwrap();

    let row = images_row(dir.path(), &lib.join("burst-1.jpg")).unwrap();
    assert_eq!(row.face_count, Some(2), "face facts persisted");

    // Reclustering re-scores with the new signal.
    let reclustered = engine.cluster_bursts(3, 0.92).unwrap();
    assert_eq!(reclustered.len(), 1);
    let keeper = engine.clusters(Some("burst")).unwrap()[0]
        .keeper_file_id
        .unwrap();
    assert_ne!(keeper, old_keeper, "an eyes-open shot becomes the keeper");
}
