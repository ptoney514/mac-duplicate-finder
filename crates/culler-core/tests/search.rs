//! Semantic search plumbing end to end with a stub embedder: embedding
//! stage during scan, persistence, index maintenance, ranked queries.

mod common;

use common::{open_engine, scan, solid_jpeg, StubEmbedder, TempDir};

#[test]
fn scan_embeds_and_search_ranks_by_meaning() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    solid_jpeg(&lib, "cherry.jpg", [220, 20, 20], 64);
    solid_jpeg(&lib, "ocean.jpg", [20, 20, 220], 64);
    let mut engine = open_engine(dir.path());
    engine.attach_embedder(Box::new(StubEmbedder));

    let summary = scan(&mut engine, &lib);
    assert_eq!(summary.embedded, 2);

    let hits = engine.search("red", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].path.ends_with("cherry.jpg"), "{hits:?}");
    assert!(hits[0].score > hits[1].score);
    assert!(hits[0].thumb_path.is_some());

    let hits = engine.search("blue", 10).unwrap();
    assert!(hits[0].path.ends_with("ocean.jpg"), "{hits:?}");
}

#[test]
fn embedding_is_incremental() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    solid_jpeg(&lib, "a.jpg", [200, 30, 30], 64);
    let mut engine = open_engine(dir.path());
    engine.attach_embedder(Box::new(StubEmbedder));

    assert_eq!(scan(&mut engine, &lib).embedded, 1);
    assert_eq!(scan(&mut engine, &lib).embedded, 0, "already embedded");

    solid_jpeg(&lib, "b.jpg", [30, 200, 30], 96);
    assert_eq!(scan(&mut engine, &lib).embedded, 1, "only the new file");
}

#[test]
fn scan_without_models_embeds_nothing_and_search_errors() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    solid_jpeg(&lib, "a.jpg", [200, 30, 30], 64);
    let mut engine = open_engine(dir.path());

    assert_eq!(scan(&mut engine, &lib).embedded, 0);
    assert!(engine.search("red", 5).is_err(), "no models attached");
}

#[test]
fn index_file_is_rebuilt_from_stored_embeddings() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    solid_jpeg(&lib, "cherry.jpg", [220, 20, 20], 64);
    solid_jpeg(&lib, "ocean.jpg", [20, 20, 220], 64);
    let mut engine = open_engine(dir.path());
    engine.attach_embedder(Box::new(StubEmbedder));
    scan(&mut engine, &lib);

    let index_file = dir.path().join("culler.usearch");
    assert!(index_file.exists(), "index persisted after scan");
    std::fs::remove_file(&index_file).unwrap();

    // Fresh engine, no index file: search falls back to the embeddings
    // table and rewrites the index.
    let mut engine = open_engine(dir.path());
    engine.attach_embedder(Box::new(StubEmbedder));
    let hits = engine.search("blue", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].path.ends_with("ocean.jpg"));
    assert!(index_file.exists(), "index rebuilt on demand");
}

#[test]
fn deleted_files_leave_search_results() {
    let dir = TempDir::new();
    let lib = dir.path().join("lib");
    solid_jpeg(&lib, "cherry.jpg", [220, 20, 20], 64);
    let ocean = solid_jpeg(&lib, "ocean.jpg", [20, 20, 220], 64);
    let mut engine = open_engine(dir.path());
    engine.attach_embedder(Box::new(StubEmbedder));
    scan(&mut engine, &lib);

    std::fs::remove_file(&ocean).unwrap();
    scan(&mut engine, &lib);

    let hits = engine.search("blue", 10).unwrap();
    assert!(
        hits.iter().all(|h| !h.path.ends_with("ocean.jpg")),
        "missing files are filtered out: {hits:?}"
    );
}
