//! SQLite persistence, WAL mode. This database is the source of truth for
//! file facts (section 6 of the PRD).

pub mod migrations;

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::analyze::ImageFacts;
use crate::cluster::near::HashedImage;
use crate::{CoreError, DupeFile, DupeGroup, FoundFile, NearCluster, Result};

pub struct Db {
    conn: Connection,
}

/// Outcome of recording one walk of a root.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RecordStats {
    pub added: u64,
    pub updated: u64,
    pub unchanged: u64,
    pub missing: u64,
}

/// A file that shares its size with at least one other live file, making it
/// an exact-duplicate candidate.
#[derive(Debug, Clone)]
pub struct CandidateFile {
    pub id: i64,
    pub path: String,
    pub already_hashed: bool,
}

/// A live file with no `images` row yet.
#[derive(Debug, Clone)]
pub struct AnalysisTarget {
    pub id: i64,
    pub path: String,
    pub content_hash: Option<[u8; 32]>,
}

impl Db {
    /// Opens (creating if needed) the database, enables WAL, runs migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                    path: parent.display().to_string(),
                    source,
                })?;
            }
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations::run(&conn)?;
        Ok(Self { conn })
    }

    /// Incrementally records one walk of `root`:
    /// - new paths are inserted as `pending`;
    /// - rows whose size and mtime are unchanged are left alone;
    /// - changed rows get the new size/mtime, a cleared hash, and `pending`;
    /// - rows under `root` not in `found` are marked `missing`;
    /// - previously-missing rows that reappeared unchanged get their old
    ///   status back (`hashed` if a hash is stored, else `pending`).
    pub fn record_found(&mut self, root: &str, found: &[FoundFile]) -> Result<RecordStats> {
        let tx = self.conn.transaction()?;
        let mut stats = RecordStats::default();
        {
            let mut select =
                tx.prepare("SELECT id, size, mtime, status FROM files WHERE path = ?1")?;
            let mut insert = tx.prepare(
                "INSERT INTO files (path, size, mtime, status) VALUES (?1, ?2, ?3, 'pending')",
            )?;
            let mut update = tx.prepare(
                "UPDATE files SET size = ?2, mtime = ?3, content_hash = NULL, \
                 status = 'pending' WHERE path = ?1",
            )?;
            let mut revive = tx.prepare(
                "UPDATE files SET status = CASE WHEN content_hash IS NULL \
                 THEN 'pending' ELSE 'hashed' END WHERE path = ?1",
            )?;
            // Changed content invalidates every derived fact.
            let mut drop_embedding = tx.prepare("DELETE FROM embeddings WHERE file_id = ?1")?;
            let mut drop_image = tx.prepare("DELETE FROM images WHERE file_id = ?1")?;
            for f in found {
                let existing = select
                    .query_row(params![f.path], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })
                    .optional()?;
                match existing {
                    None => {
                        insert.execute(params![f.path, f.size as i64, f.mtime])?;
                        stats.added += 1;
                    }
                    Some((_, size, mtime, status)) if size == f.size as i64 && mtime == f.mtime => {
                        if status == "missing" {
                            revive.execute(params![f.path])?;
                            stats.updated += 1;
                        } else {
                            stats.unchanged += 1;
                        }
                    }
                    Some((id, _, _, _)) => {
                        update.execute(params![f.path, f.size as i64, f.mtime])?;
                        drop_embedding.execute([id])?;
                        drop_image.execute([id])?;
                        stats.updated += 1;
                    }
                }
            }
        }

        // Mark rows under this root that the walk no longer saw.
        let prefix = if root.ends_with('/') {
            root.to_owned()
        } else {
            format!("{root}/")
        };
        let found_paths: HashSet<&str> = found.iter().map(|f| f.path.as_str()).collect();
        let mut newly_missing = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT id, path FROM files WHERE status != 'missing'")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, path) = row?;
                if path.starts_with(&prefix) && !found_paths.contains(path.as_str()) {
                    newly_missing.push(id);
                }
            }
        }
        {
            let mut mark = tx.prepare("UPDATE files SET status = 'missing' WHERE id = ?1")?;
            for id in &newly_missing {
                mark.execute([id])?;
            }
        }
        stats.missing = newly_missing.len() as u64;
        tx.commit()?;
        Ok(stats)
    }

    /// Groups of live (non-missing) files sharing a size, for sizes with at
    /// least two files. Size-first candidate grouping for hashing.
    pub fn size_candidates(&self) -> Result<Vec<(u64, Vec<CandidateFile>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT size, id, path, content_hash IS NOT NULL FROM files \
             WHERE status != 'missing' AND size IN ( \
                 SELECT size FROM files WHERE status != 'missing' \
                 GROUP BY size HAVING COUNT(*) >= 2) \
             ORDER BY size",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as u64,
                CandidateFile {
                    id: row.get(1)?,
                    path: row.get(2)?,
                    already_hashed: row.get(3)?,
                },
            ))
        })?;
        let mut groups: Vec<(u64, Vec<CandidateFile>)> = Vec::new();
        for row in rows {
            let (size, file) = row?;
            match groups.last_mut() {
                Some((s, members)) if *s == size => members.push(file),
                _ => groups.push((size, vec![file])),
            }
        }
        Ok(groups)
    }

    /// Stores content hashes and marks the rows `hashed`.
    pub fn store_hashes(&mut self, hashes: &[(i64, [u8; 32])]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("UPDATE files SET content_hash = ?2, status = 'hashed' WHERE id = ?1")?;
            for (id, hash) in hashes {
                stmt.execute(params![id, &hash[..]])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Live files that have no `images` row yet (never analyzed, or their
    /// content changed and the stale row was dropped).
    pub fn files_needing_analysis(&self) -> Result<Vec<AnalysisTarget>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.path, f.content_hash FROM files f \
             LEFT JOIN images i ON i.file_id = f.id \
             WHERE f.status != 'missing' AND i.file_id IS NULL \
             ORDER BY f.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AnalysisTarget {
                id: row.get(0)?,
                path: row.get(1)?,
                content_hash: row
                    .get::<_, Option<Vec<u8>>>(2)?
                    .map(|blob| blob.try_into().expect("content_hash is 32 bytes")),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Writes analysis results: the content hash for files first hashed
    /// during analysis, the `images` row, and status `analyzed`.
    pub fn store_analysis(
        &mut self,
        results: &[(i64, Option<[u8; 32]>, ImageFacts)],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut set_hash = tx
                .prepare("UPDATE files SET content_hash = ?2, status = 'analyzed' WHERE id = ?1")?;
            let mut set_status =
                tx.prepare("UPDATE files SET status = 'analyzed' WHERE id = ?1")?;
            let mut upsert = tx.prepare(
                "INSERT OR REPLACE INTO images (file_id, width, height, captured_at, \
                 camera, orientation, dhash, phash, thumb_path, sharpness, exposure_score) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for (id, new_hash, facts) in results {
                match new_hash {
                    Some(hash) => set_hash.execute(params![id, &hash[..]])?,
                    None => set_status.execute([id])?,
                };
                upsert.execute(params![
                    id,
                    facts.width,
                    facts.height,
                    facts.captured_at,
                    facts.camera,
                    facts.orientation,
                    facts.dhash.map(|h| h as i64),
                    facts.phash.map(|h| h as i64),
                    facts.thumb_path,
                    facts.sharpness,
                    facts.exposure_score,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Perceptual hashes of live analyzed images, one representative per
    /// content hash (smallest file id): exact duplicates belong to the dupes
    /// flow, not near clusters.
    pub fn perceptual_hashes(&self) -> Result<Vec<HashedImage>> {
        let mut stmt = self.conn.prepare(
            "SELECT i.file_id, i.dhash, i.phash FROM images i \
             JOIN files f ON f.id = i.file_id \
             WHERE f.status != 'missing' AND f.id = ( \
                 SELECT MIN(f2.id) FROM files f2 \
                 WHERE f2.content_hash = f.content_hash AND f2.status != 'missing') \
             ORDER BY i.file_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(HashedImage {
                id: row.get(0)?,
                dhash: row.get::<_, Option<i64>>(1)?.map(|h| h as u64),
                phash: row.get::<_, Option<i64>>(2)?.map(|h| h as u64),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Replaces all clusters of `kind` with `components` (lists of file ids)
    /// and returns them with member paths, preserving component order.
    pub fn replace_clusters(
        &mut self,
        kind: &str,
        components: &[Vec<i64>],
    ) -> Result<Vec<NearCluster>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let tx = self.conn.transaction()?;
        let mut out = Vec::with_capacity(components.len());
        {
            tx.execute(
                "DELETE FROM cluster_members WHERE cluster_id IN \
                 (SELECT id FROM clusters WHERE kind = ?1)",
                [kind],
            )?;
            tx.execute("DELETE FROM clusters WHERE kind = ?1", [kind])?;
            let mut insert_cluster =
                tx.prepare("INSERT INTO clusters (kind, created_at) VALUES (?1, ?2)")?;
            let mut insert_member =
                tx.prepare("INSERT INTO cluster_members (cluster_id, file_id) VALUES (?1, ?2)")?;
            let mut path_of = tx.prepare("SELECT path FROM files WHERE id = ?1")?;
            for component in components {
                insert_cluster.execute(params![kind, now])?;
                let cluster_id = tx.last_insert_rowid();
                let mut files = Vec::with_capacity(component.len());
                for file_id in component {
                    insert_member.execute(params![cluster_id, file_id])?;
                    files.push(path_of.query_row([file_id], |row| row.get::<_, String>(0))?);
                }
                files.sort();
                out.push(NearCluster {
                    id: cluster_id,
                    files,
                });
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// Live analyzed images with a thumbnail but no stored embedding yet.
    /// Thumbnails are the embedding source (ADR-0004).
    pub fn files_needing_embedding(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, i.thumb_path FROM images i \
             JOIN files f ON f.id = i.file_id \
             LEFT JOIN embeddings e ON e.file_id = f.id \
             WHERE f.status != 'missing' AND i.thumb_path IS NOT NULL \
               AND e.file_id IS NULL \
             ORDER BY f.id",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Stores 512-dim f32 vectors as little-endian blobs (PRD §6).
    pub fn store_embeddings(&mut self, rows: &[(i64, Vec<f32>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT OR REPLACE INTO embeddings (file_id, vector) VALUES (?1, ?2)")?;
            for (id, vector) in rows {
                let blob: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
                stmt.execute(params![id, blob])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn embeddings_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Every stored embedding, decoded. Used to rebuild the vector index.
    pub fn embedding_rows(&self) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_id, vector FROM embeddings ORDER BY file_id")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            let vector = blob
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            out.push((id, vector));
        }
        Ok(out)
    }

    /// Writes aesthetic scores onto existing images rows.
    pub fn store_aesthetics(&mut self, rows: &[(i64, f64)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("UPDATE images SET aesthetic_score = ?2 WHERE file_id = ?1")?;
            for (id, score) in rows {
                stmt.execute(params![id, score])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Writes Apple Vision face facts onto existing images rows.
    pub fn store_face_facts(&mut self, facts: &[(i64, u32, f64)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE images SET face_count = ?2, eyes_open_ratio = ?3 WHERE file_id = ?1",
            )?;
            for (id, count, ratio) in facts {
                stmt.execute(params![id, count, ratio])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Analyzed live images that have no face facts yet but do have a
    /// thumbnail the Swift Vision pass can read.
    pub fn images_needing_faces(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, i.thumb_path FROM images i JOIN files f ON f.id = i.file_id \
             WHERE f.status != 'missing' AND i.thumb_path IS NOT NULL \
               AND i.face_count IS NULL ORDER BY f.id",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Candidate frames for burst clustering: one representative per content
    /// hash (like near clustering), with camera, capture time, embedding.
    pub fn burst_frames(&self) -> Result<Vec<crate::cluster::burst::BurstFrame>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, i.camera, i.captured_at, e.vector \
             FROM images i JOIN files f ON f.id = i.file_id \
             LEFT JOIN embeddings e ON e.file_id = f.id \
             WHERE f.status != 'missing' AND f.id = ( \
                 SELECT MIN(f2.id) FROM files f2 \
                 WHERE f2.content_hash = f.content_hash AND f2.status != 'missing') \
             ORDER BY f.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, camera, captured_at, blob) = row?;
            let embedding = blob.map(|b| {
                b.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect()
            });
            out.push(crate::cluster::burst::BurstFrame {
                id,
                camera,
                captured_at,
                embedding,
            });
        }
        Ok(out)
    }

    /// Ids of every stored cluster.
    pub fn cluster_ids(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM clusters ORDER BY id")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Stored §7 signals for one cluster's members, in member id order.
    pub fn cluster_member_signals(
        &self,
        cluster_id: i64,
    ) -> Result<Vec<crate::cluster::scoring::MemberSignals>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, i.sharpness, i.exposure_score, i.aesthetic_score, \
                    i.face_count, i.eyes_open_ratio, i.width, i.height \
             FROM cluster_members cm \
             JOIN files f ON f.id = cm.file_id \
             LEFT JOIN images i ON i.file_id = f.id \
             WHERE cm.cluster_id = ?1 ORDER BY f.id",
        )?;
        let rows = stmt.query_map([cluster_id], |row| {
            let width: Option<i64> = row.get(6)?;
            let height: Option<i64> = row.get(7)?;
            Ok(crate::cluster::scoring::MemberSignals {
                id: row.get(0)?,
                sharpness: row.get(1)?,
                exposure: row.get(2)?,
                aesthetic: row.get(3)?,
                face_count: row.get(4)?,
                eyes_open_ratio: row.get(5)?,
                pixels: width
                    .zip(height)
                    .map(|(w, h)| (w.max(0) as u64) * (h.max(0) as u64)),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Writes composite quality scores and one cluster's keeper.
    pub fn store_cluster_scores(
        &mut self,
        cluster_id: i64,
        keeper_file_id: Option<i64>,
        scores: &[(i64, f64)],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut set_quality =
                tx.prepare("UPDATE images SET quality_score = ?2 WHERE file_id = ?1")?;
            for (id, score) in scores {
                set_quality.execute(params![id, score])?;
            }
            tx.execute(
                "UPDATE clusters SET keeper_file_id = ?2 WHERE id = ?1",
                params![cluster_id, keeper_file_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Stored clusters with member metadata, optionally filtered by kind.
    /// Members in filmstrip order: capture time ascending (nulls last), id.
    pub fn clusters_with_members(&self, kind: Option<&str>) -> Result<Vec<crate::ClusterDetail>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, keeper_file_id FROM clusters \
             WHERE ?1 IS NULL OR kind = ?1 ORDER BY id",
        )?;
        let headers = stmt
            .query_map([kind], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut member_stmt = self.conn.prepare(
            "SELECT f.id, f.path, i.thumb_path, i.quality_score, f.content_hash, i.captured_at \
             FROM cluster_members cm \
             JOIN files f ON f.id = cm.file_id \
             LEFT JOIN images i ON i.file_id = f.id \
             WHERE cm.cluster_id = ?1 \
             ORDER BY i.captured_at IS NULL, i.captured_at, f.id",
        )?;
        let mut out = Vec::with_capacity(headers.len());
        for (id, kind, keeper_file_id) in headers {
            let members = member_stmt
                .query_map([id], |row| {
                    let hash: Option<Vec<u8>> = row.get(4)?;
                    Ok(crate::ClusterMember {
                        file_id: row.get(0)?,
                        path: row.get(1)?,
                        thumb_path: row.get(2)?,
                        quality_score: row.get(3)?,
                        content_hash_hex: hash
                            .map(|h| h.iter().map(|b| format!("{b:02x}")).collect())
                            .unwrap_or_default(),
                        captured_at: row.get(5)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.push(crate::ClusterDetail {
                id,
                kind,
                keeper_file_id,
                members,
            });
        }
        Ok(out)
    }

    /// Path, thumbnail, and liveness for one file id (search result lookup).
    pub fn file_meta(&self, id: i64) -> Result<Option<(String, Option<String>, bool)>> {
        self.conn
            .query_row(
                "SELECT f.path, i.thumb_path, f.status != 'missing' FROM files f \
                 LEFT JOIN images i ON i.file_id = f.id WHERE f.id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Analyzed live images, newest capture first (NULL `captured_at` last,
    /// path as tiebreak), paginated for the library grid.
    pub fn grid_items(&self, offset: u64, limit: u64) -> Result<Vec<crate::GridItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.path, i.thumb_path, i.captured_at, i.width, i.height \
             FROM images i JOIN files f ON f.id = i.file_id \
             WHERE f.status != 'missing' \
             ORDER BY i.captured_at IS NULL, i.captured_at DESC, f.path \
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
            Ok(crate::GridItem {
                file_id: row.get(0)?,
                path: row.get(1)?,
                thumb_path: row.get(2)?,
                captured_at: row.get(3)?,
                width: row.get::<_, Option<i64>>(4)?.map(|w| w as u32),
                height: row.get::<_, Option<i64>>(5)?.map(|h| h as u32),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Exact-duplicate groups among live hashed files, sorted by reclaimable
    /// bytes descending. Members are sorted keeper-first (oldest mtime, then
    /// shortest path, then path).
    pub fn dupe_groups(&self) -> Result<Vec<DupeGroup>> {
        let mut stmt = self.conn.prepare(
            "SELECT content_hash, size, path, mtime FROM files \
             WHERE status != 'missing' AND content_hash IS NOT NULL \
             ORDER BY content_hash",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, i64>(1)? as u64,
                DupeFile {
                    path: row.get(2)?,
                    mtime: row.get(3)?,
                },
            ))
        })?;

        let mut groups: Vec<DupeGroup> = Vec::new();
        for row in rows {
            let (hash_blob, size, file) = row?;
            let hash: [u8; 32] = hash_blob
                .try_into()
                .expect("content_hash is always 32 bytes");
            match groups.last_mut() {
                Some(g) if g.hash == hash => g.files.push(file),
                _ => groups.push(DupeGroup {
                    hash,
                    size,
                    files: vec![file],
                    reclaimable: 0,
                }),
            }
        }
        groups.retain(|g| g.files.len() >= 2);
        for g in &mut groups {
            g.files.sort_by(|a, b| {
                (a.mtime, a.path.len(), &a.path).cmp(&(b.mtime, b.path.len(), &b.path))
            });
            g.reclaimable = g.size * (g.files.len() as u64 - 1);
        }
        groups.sort_by(|a, b| {
            b.reclaimable
                .cmp(&a.reclaimable)
                .then_with(|| a.files[0].path.cmp(&b.files[0].path))
        });
        Ok(groups)
    }
}
