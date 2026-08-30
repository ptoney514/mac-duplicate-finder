//! Schema migration tests: fresh databases reach the current version, and
//! reopening an existing database is a safe no-op.

mod common;

use common::TempDir;
use culler_core::Engine;

fn user_version(conn: &rusqlite::Connection) -> i32 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn table_names(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

#[test]
fn fresh_db_gets_current_schema_in_wal_mode() {
    let dir = TempDir::new();
    let db_path = dir.path().join("culler.db");

    Engine::open(&db_path).unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    assert_eq!(user_version(&conn), 1);
    let tables = table_names(&conn);
    for expected in [
        "cluster_members",
        "clusters",
        "embeddings",
        "files",
        "frontier_verdicts",
        "images",
    ] {
        assert!(
            tables.iter().any(|t| t == expected),
            "missing table {expected}, have {tables:?}"
        );
    }
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
}

#[test]
fn reopening_preserves_data_and_version() {
    let dir = TempDir::new();
    let db_path = dir.path().join("culler.db");

    Engine::open(&db_path).unwrap();
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO files (path, size, mtime, status) VALUES ('/x.jpg', 1, 1, 'pending')",
            [],
        )
        .unwrap();
    }

    // Second open must not re-run migration 1 (which would fail or wipe data).
    Engine::open(&db_path).unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    assert_eq!(user_version(&conn), 1);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}
