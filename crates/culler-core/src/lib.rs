//! Culler engine: file scanning, hashing, analysis, clustering, persistence.
//!
//! Milestone 1 implements the scan pipeline (walk → record → hash) and exact
//! duplicate lookup. Later milestones fill in `analyze`, `embed`, `index`,
//! and `cluster`.

pub mod analyze;
pub mod api;
pub mod cluster;
pub mod scan;

uniffi::setup_scaffolding!();

mod db;
mod embed;
mod index;

use std::collections::HashMap;
use std::path::Path;

use analyze::ImageFacts;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, CoreError>;

/// A file discovered by the walker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFile {
    /// Absolute path.
    pub path: String,
    pub size: u64,
    /// Modification time, unix seconds.
    pub mtime: i64,
}

/// Progress events emitted during [`Engine::scan`].
#[derive(Debug, Clone, Copy)]
pub enum ScanProgress {
    Walking { found: u64 },
    Hashing { done: u64, total: u64 },
    Analyzing { done: u64, total: u64 },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanSummary {
    /// Image files seen on disk under the root.
    pub found: u64,
    /// New rows inserted.
    pub added: u64,
    /// Rows whose size/mtime changed (or that reappeared after going missing).
    pub updated: u64,
    /// Rows skipped because path, size, and mtime were unchanged.
    pub unchanged: u64,
    /// Rows under the root no longer present on disk.
    pub missing: u64,
    /// Files content-hashed during this scan's candidate stage (size-first
    /// grouping). Files first hashed during analysis are not counted here.
    pub hashed: u64,
    /// Files analyzed during this scan (EXIF, thumbnail, perceptual hashes).
    pub analyzed: u64,
    /// Files or directories skipped due to I/O errors.
    pub errors: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DupeFile {
    pub path: String,
    pub mtime: i64,
}

/// A set of files with identical BLAKE3 content hashes.
#[derive(Debug, Clone)]
pub struct DupeGroup {
    pub hash: [u8; 32],
    pub size: u64,
    /// Members sorted keeper-first: oldest mtime, then shortest path, then path.
    pub files: Vec<DupeFile>,
    /// Bytes freed if everything but the keeper were removed.
    pub reclaimable: u64,
}

/// Handle to the engine and its database. One per library.
pub struct Engine {
    db: db::Db,
    /// Thumbnail cache, sibling of the database file.
    thumbs_dir: std::path::PathBuf,
}

impl Engine {
    /// Opens (creating if needed) the database at `db_path` and runs migrations.
    pub fn open(db_path: &Path) -> Result<Self> {
        Ok(Self {
            db: db::Db::open(db_path)?,
            thumbs_dir: db_path.parent().unwrap_or(Path::new(".")).join("thumbs"),
        })
    }

    /// Walks `root`, records file facts incrementally, and content-hashes
    /// exact-duplicate candidates (size-first grouping, 4 KB prehash, then
    /// full BLAKE3). Files whose path, size, and mtime are unchanged since
    /// the last scan are skipped.
    pub fn scan(
        &mut self,
        root: &Path,
        on_progress: &mut dyn FnMut(ScanProgress),
    ) -> Result<ScanSummary> {
        use rayon::prelude::*;

        let root = root.canonicalize().map_err(|source| CoreError::Io {
            path: root.display().to_string(),
            source,
        })?;
        let walk = scan::walker::walk_images(&root, &mut |found| {
            on_progress(ScanProgress::Walking { found });
        })?;
        let mut summary = ScanSummary {
            found: walk.files.len() as u64,
            errors: walk.errors,
            ..Default::default()
        };
        let recorded = self.db.record_found(&root.to_string_lossy(), &walk.files)?;
        summary.added = recorded.added;
        summary.updated = recorded.updated;
        summary.unchanged = recorded.unchanged;
        summary.missing = recorded.missing;

        // Plan: within each same-size group that has unhashed members,
        // prehash everyone (4 KB reads), then full-hash only the members of
        // multi-file prehash subgroups that still lack a hash.
        let mut full_targets: Vec<db::CandidateFile> = Vec::new();
        for (_size, members) in self.db.size_candidates()? {
            if members.iter().all(|m| m.already_hashed) {
                continue;
            }
            let prehashes: Vec<(usize, std::io::Result<[u8; 32]>)> = members
                .par_iter()
                .enumerate()
                .map(|(i, m)| (i, scan::hash::prehash_file(Path::new(&m.path))))
                .collect();
            let mut by_prehash: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
            for (i, result) in prehashes {
                match result {
                    Ok(prehash) => by_prehash.entry(prehash).or_default().push(i),
                    Err(_) => summary.errors += 1,
                }
            }
            for indices in by_prehash.into_values() {
                if indices.len() < 2 {
                    continue;
                }
                full_targets.extend(
                    indices
                        .into_iter()
                        .map(|i| &members[i])
                        .filter(|m| !m.already_hashed)
                        .cloned(),
                );
            }
        }

        // Full hashes in parallel batches so progress stays live.
        let total = full_targets.len() as u64;
        if total > 0 {
            on_progress(ScanProgress::Hashing { done: 0, total });
        }
        let mut hashes = Vec::with_capacity(full_targets.len());
        let mut done = 0u64;
        for batch in full_targets.chunks(64) {
            let results: Vec<(i64, std::io::Result<[u8; 32]>)> = batch
                .par_iter()
                .map(|m| (m.id, scan::hash::hash_file(Path::new(&m.path))))
                .collect();
            for (id, result) in results {
                match result {
                    Ok(hash) => hashes.push((id, hash)),
                    Err(_) => summary.errors += 1,
                }
            }
            done += batch.len() as u64;
            on_progress(ScanProgress::Hashing { done, total });
        }
        self.db.store_hashes(&hashes)?;
        summary.hashed = hashes.len() as u64;

        // Analysis: EXIF, thumbnail, perceptual hashes for every live file
        // without an images row. Files the candidate stage skipped get their
        // content hash here (the thumbnail cache is keyed by it).
        let targets = self.db.files_needing_analysis()?;
        let total = targets.len() as u64;
        let mut done = 0u64;
        if total > 0 {
            on_progress(ScanProgress::Analyzing { done: 0, total });
        }
        for batch in targets.chunks(16) {
            type AnalysisResult = (i64, std::io::Result<(Option<[u8; 32]>, ImageFacts, bool)>);
            let results: Vec<AnalysisResult> = batch
                .par_iter()
                .map(|target| {
                    let hash = match target.content_hash {
                        Some(hash) => hash,
                        None => match scan::hash::hash_file(Path::new(&target.path)) {
                            Ok(hash) => hash,
                            Err(e) => return (target.id, Err(e)),
                        },
                    };
                    let new_hash = target.content_hash.is_none().then_some(hash);
                    let (facts, soft_error) =
                        analyze::analyze_file(Path::new(&target.path), &hash, &self.thumbs_dir);
                    (target.id, Ok((new_hash, facts, soft_error)))
                })
                .collect();
            let mut to_store = Vec::with_capacity(results.len());
            for (id, result) in results {
                match result {
                    Ok((new_hash, facts, soft_error)) => {
                        summary.errors += u64::from(soft_error);
                        to_store.push((id, new_hash, facts));
                    }
                    // Unreadable file: leave it pending and retry next scan.
                    Err(_) => summary.errors += 1,
                }
            }
            summary.analyzed += to_store.len() as u64;
            self.db.store_analysis(&to_store)?;
            done += batch.len() as u64;
            on_progress(ScanProgress::Analyzing { done, total });
        }
        Ok(summary)
    }

    /// Exact-duplicate groups, sorted by reclaimable bytes descending.
    pub fn dupes(&self) -> Result<Vec<DupeGroup>> {
        self.db.dupe_groups()
    }

    /// Rebuilds near-duplicate clusters from the stored perceptual hashes
    /// (no re-analysis) and returns them, largest first. Two images are near
    /// dupes when their dHash Hamming distance is <= `dhash_max` OR their
    /// pHash distance is <= `phash_max` (PRD section 8); clusters are the
    /// connected components of that relation.
    pub fn cluster_near(&mut self, dhash_max: u32, phash_max: u32) -> Result<Vec<NearCluster>> {
        let hashes = self.db.perceptual_hashes()?;
        let components = cluster::near::near_components(&hashes, dhash_max, phash_max);
        self.db.replace_near_clusters(&components)
    }

    /// Analyzed live images for the library grid, newest capture first
    /// (undated images last, path as tiebreak), paginated.
    pub fn grid_items(&self, offset: u64, limit: u64) -> Result<Vec<GridItem>> {
        self.db.grid_items(offset, limit)
    }
}

/// A stored cluster of near-duplicate images.
#[derive(Debug, Clone)]
pub struct NearCluster {
    pub id: i64,
    /// Member paths, sorted.
    pub files: Vec<String>,
}

/// One cell of the library grid: an analyzed image and its cached thumbnail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridItem {
    pub file_id: i64,
    pub path: String,
    pub thumb_path: Option<String>,
    pub captured_at: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
