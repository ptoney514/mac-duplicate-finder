//! Versioned schema migrations driven by `PRAGMA user_version`.

use rusqlite::Connection;

/// Schema version this build writes.
pub const CURRENT_VERSION: i32 = 1;

/// Applies any migrations newer than the database's `user_version`.
/// Idempotent: reopening an up-to-date database is a no-op.
pub fn run(conn: &Connection) -> rusqlite::Result<()> {
    let mut version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    while version < CURRENT_VERSION {
        match version {
            0 => conn.execute_batch(V1)?,
            v => unreachable!("no migration defined from schema version {v}"),
        }
        version += 1;
        conn.pragma_update(None, "user_version", version)?;
    }
    Ok(())
}

/// Initial schema, PRD section 6. `clusters.kind` is unconstrained because
/// section 8 adds a "possible" kind beyond the ones listed in the table.
const V1: &str = "
BEGIN;

CREATE TABLE files (
    id            INTEGER PRIMARY KEY,
    path          TEXT NOT NULL UNIQUE,
    size          INTEGER NOT NULL,
    mtime         INTEGER NOT NULL,
    content_hash  BLOB,
    status        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'hashed', 'analyzed', 'missing'))
);
CREATE INDEX idx_files_size ON files (size);
CREATE INDEX idx_files_content_hash ON files (content_hash)
    WHERE content_hash IS NOT NULL;

CREATE TABLE images (
    file_id         INTEGER PRIMARY KEY REFERENCES files (id),
    width           INTEGER,
    height          INTEGER,
    captured_at     INTEGER,
    camera          TEXT,
    orientation     INTEGER,
    dhash           INTEGER,
    phash           INTEGER,
    sharpness       REAL,
    exposure_score  REAL,
    face_count      INTEGER,
    eyes_open_ratio REAL,
    aesthetic_score REAL,
    quality_score   REAL,
    thumb_path      TEXT
);

CREATE TABLE embeddings (
    file_id INTEGER PRIMARY KEY REFERENCES images (file_id),
    vector  BLOB NOT NULL
);

CREATE TABLE clusters (
    id             INTEGER PRIMARY KEY,
    kind           TEXT NOT NULL,
    keeper_file_id INTEGER REFERENCES files (id),
    created_at     INTEGER NOT NULL
);

CREATE TABLE cluster_members (
    cluster_id INTEGER NOT NULL REFERENCES clusters (id),
    file_id    INTEGER NOT NULL REFERENCES files (id),
    rank       INTEGER,
    PRIMARY KEY (cluster_id, file_id)
);

CREATE TABLE frontier_verdicts (
    cluster_id     INTEGER PRIMARY KEY REFERENCES clusters (id),
    model          TEXT NOT NULL,
    keeper_file_id INTEGER REFERENCES files (id),
    reason         TEXT,
    created_at     INTEGER NOT NULL
);

COMMIT;
";
