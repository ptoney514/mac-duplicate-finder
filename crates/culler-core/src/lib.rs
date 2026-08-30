//! Culler engine: file scanning, hashing, analysis, clustering, persistence.
//!
//! Milestone 1 implements the scan pipeline (walk → record → hash) and exact
//! duplicate lookup. Later milestones fill in `analyze`, `embed`, `index`,
//! and `cluster`.

pub mod analyze;
pub mod api;
pub mod cluster;
pub mod embed;
pub mod index;
pub mod scan;

uniffi::setup_scaffolding!();

mod db;

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
    /// Embedding model, tokenizer, or vector index problems.
    #[error("{message}")]
    Model { message: String },
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
    Embedding { done: u64, total: u64 },
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
    /// Images embedded during this scan (0 unless models are attached).
    pub embedded: u64,
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

/// One semantic-search hit.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file_id: i64,
    pub path: String,
    pub thumb_path: Option<String>,
    /// Cosine similarity in [-1, 1]; higher is better.
    pub score: f32,
}

/// Handle to the engine and its database. One per library.
pub struct Engine {
    db: db::Db,
    /// Thumbnail cache, sibling of the database file.
    thumbs_dir: std::path::PathBuf,
    /// Vector index file, sibling of the database file.
    index_path: std::path::PathBuf,
    embedder: Option<Box<dyn embed::Embedder>>,
    aesthetic: Option<embed::aesthetic::AestheticHead>,
    index: Option<index::VectorIndex>,
    quality_weights: cluster::scoring::QualityWeights,
}

impl Engine {
    /// Opens (creating if needed) the database at `db_path` and runs migrations.
    pub fn open(db_path: &Path) -> Result<Self> {
        let parent = db_path.parent().unwrap_or(Path::new("."));
        Ok(Self {
            db: db::Db::open(db_path)?,
            thumbs_dir: parent.join("thumbs"),
            index_path: parent.join("culler.usearch"),
            embedder: None,
            aesthetic: None,
            index: None,
            quality_weights: cluster::scoring::QualityWeights::default(),
        })
    }

    /// Attaches an embedder; subsequent scans embed and `search` works.
    pub fn attach_embedder(&mut self, embedder: Box<dyn embed::Embedder>) {
        self.embedder = Some(embedder);
    }

    /// Loads the ONNX CLIP models (and the aesthetic head when installed)
    /// from `models_dir` and attaches them.
    pub fn attach_models(&mut self, models_dir: &Path) -> Result<()> {
        self.attach_embedder(Box::new(embed::onnx::OnnxEmbedder::load(models_dir)?));
        let aesthetic_path = models_dir.join(embed::aesthetic::AESTHETIC_FILE);
        if aesthetic_path.exists() {
            self.aesthetic = Some(embed::aesthetic::AestheticHead::load(&aesthetic_path)?);
        }
        Ok(())
    }

    pub fn has_embedder(&self) -> bool {
        self.embedder.is_some()
    }

    /// Semantic search: embeds `query` with the CLIP text encoder and ranks
    /// the library by cosine similarity. Requires attached models.
    pub fn search(&mut self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let mut query_vec = {
            let embedder = self.embedder.as_deref().ok_or_else(|| CoreError::Model {
                message: "no models attached; run scripts/fetch-models.sh".into(),
            })?;
            embedder.embed_text(query)?
        };
        embed::normalize(&mut query_vec);
        self.ensure_index()?;

        // Overfetch so filtering out missing files still fills the page.
        let index = self.index.as_ref().expect("ensure_index sets it");
        let hits = index.search(&query_vec, limit.max(1) * 2)?;
        let mut results = Vec::with_capacity(limit);
        for (key, score) in hits {
            if results.len() >= limit {
                break;
            }
            if let Some((path, thumb_path, live)) = self.db.file_meta(key as i64)? {
                if live {
                    results.push(SearchResult {
                        file_id: key as i64,
                        path,
                        thumb_path,
                        score,
                    });
                }
            }
        }
        Ok(results)
    }

    /// Loads (or rebuilds from the embeddings table) the vector index so it
    /// matches the database. Cheap when already in sync.
    fn ensure_index(&mut self) -> Result<()> {
        let count = self.db.embeddings_count()?;
        if let Some(index) = &self.index {
            if index.len() as u64 == count {
                return Ok(());
            }
        }
        if self.index_path.exists() {
            if let Ok(index) = index::VectorIndex::load(&self.index_path) {
                if index.len() as u64 == count {
                    self.index = Some(index);
                    return Ok(());
                }
            }
        }
        let mut index = index::VectorIndex::new()?;
        for (id, vector) in self.db.embedding_rows()? {
            index.add(id as u64, &vector)?;
        }
        index.save(&self.index_path)?;
        self.index = Some(index);
        Ok(())
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

        // Embedding: CLIP vectors from cached thumbnails (ADR-0004), only
        // when models are attached. New vectors go straight into the index.
        if self.embedder.is_some() {
            let targets = self.db.files_needing_embedding()?;
            let total = targets.len() as u64;
            if total > 0 {
                self.ensure_index()?;
                on_progress(ScanProgress::Embedding { done: 0, total });
                let mut done = 0u64;
                for batch in targets.chunks(8) {
                    let embedder = self.embedder.as_deref().expect("checked above");
                    let mut rows = Vec::with_capacity(batch.len());
                    for (id, thumb_path) in batch {
                        match image::open(thumb_path) {
                            Ok(img) => match embedder.embed_image(&img) {
                                Ok(vector) => rows.push((*id, vector)),
                                Err(_) => summary.errors += 1,
                            },
                            Err(_) => summary.errors += 1,
                        }
                    }
                    self.db.store_embeddings(&rows)?;
                    if let Some(head) = &self.aesthetic {
                        let aesthetics: Vec<(i64, f64)> = rows
                            .iter()
                            .map(|(id, vector)| (*id, f64::from(head.score(vector))))
                            .collect();
                        self.db.store_aesthetics(&aesthetics)?;
                    }
                    let index = self.index.as_mut().expect("ensure_index sets it");
                    for (id, vector) in &rows {
                        index.add(*id as u64, vector)?;
                    }
                    summary.embedded += rows.len() as u64;
                    done += batch.len() as u64;
                    on_progress(ScanProgress::Embedding { done, total });
                }
                self.index
                    .as_ref()
                    .expect("ensure_index sets it")
                    .save(&self.index_path)?;
            }
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
        let clusters = self.db.replace_clusters("near", &components)?;
        self.rescore_clusters()?;
        Ok(clusters)
    }

    /// Analyzed live images for the library grid, newest capture first
    /// (undated images last, path as tiebreak), paginated.
    pub fn grid_items(&self, offset: u64, limit: u64) -> Result<Vec<GridItem>> {
        self.db.grid_items(offset, limit)
    }

    /// Rebuilds burst clusters (PRD §8: same camera, captured within
    /// `max_gap_secs` of the previous frame, embedding cosine >=
    /// `min_cosine`), then re-scores quality and keepers.
    pub fn cluster_bursts(
        &mut self,
        max_gap_secs: i64,
        min_cosine: f32,
    ) -> Result<Vec<NearCluster>> {
        let frames = self.db.burst_frames()?;
        let components = cluster::burst::burst_components(&frames, max_gap_secs, min_cosine);
        let clusters = self.db.replace_clusters("burst", &components)?;
        self.rescore_clusters()?;
        Ok(clusters)
    }

    /// Stored clusters with member metadata, optionally filtered by kind.
    pub fn clusters(&self, kind: Option<&str>) -> Result<Vec<ClusterDetail>> {
        self.db.clusters_with_members(kind)
    }

    /// Stores Apple Vision face facts (count, eyes-open ratio) supplied by
    /// the Swift shell (PRD §5.3), then refreshes quality and keepers.
    pub fn store_face_facts(&mut self, facts: &[(i64, u32, f64)]) -> Result<()> {
        self.db.store_face_facts(facts)?;
        self.rescore_clusters()
    }

    /// Analyzed images the Swift Vision face pass hasn't visited yet.
    pub fn images_needing_faces(&self) -> Result<Vec<(i64, String)>> {
        self.db.images_needing_faces()
    }

    /// Recomputes §7 composite scores and keeper proposals for every stored
    /// cluster, using the configured weights.
    fn rescore_clusters(&mut self) -> Result<()> {
        for cluster_id in self.db.cluster_ids()? {
            let members = self.db.cluster_member_signals(cluster_id)?;
            let scores = cluster::scoring::composite_scores(&members, &self.quality_weights);
            let keeper = cluster::scoring::keeper_index(&members, &self.quality_weights)
                .map(|i| members[i].id);
            let pairs: Vec<(i64, f64)> = members
                .iter()
                .zip(&scores)
                .map(|(m, s)| (m.id, *s))
                .collect();
            self.db.store_cluster_scores(cluster_id, keeper, &pairs)?;
        }
        Ok(())
    }

    /// One best image per gap-based event (§9.5), chronological. Undated
    /// images never appear. Ranking: quality score, then aesthetic, then
    /// sharpness (later signals cover images outside any cluster).
    pub fn best_of(&self, gap_secs: i64) -> Result<Vec<BestOfEntry>> {
        type DatedRow = (i64, i64, Option<f64>, Option<f64>, Option<f64>);
        let dated = self.db.dated_images()?;
        let times: Vec<(i64, i64)> = dated.iter().map(|d| (d.0, d.1)).collect();
        let by_id: HashMap<i64, &DatedRow> = dated.iter().map(|d| (d.0, d)).collect();

        let rank = |id: &i64| {
            let (_, _, quality, aesthetic, sharpness) = by_id[id];
            (
                quality.unwrap_or(f64::NEG_INFINITY),
                aesthetic.unwrap_or(f64::NEG_INFINITY),
                sharpness.unwrap_or(f64::NEG_INFINITY),
            )
        };

        let mut entries = Vec::new();
        for group in cluster::gap::gap_groups(&times, gap_secs) {
            let best = *group
                .iter()
                .max_by(|a, b| {
                    rank(a)
                        .partial_cmp(&rank(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .expect("groups are non-empty");
            let Some(item) = self.db.grid_item(best)? else {
                continue;
            };
            let start = group.iter().map(|id| by_id[id].1).min().unwrap_or(0);
            let end = group.iter().map(|id| by_id[id].1).max().unwrap_or(start);
            entries.push(BestOfEntry {
                start,
                end,
                count: group.len() as u64,
                item,
            });
        }
        Ok(entries)
    }

    /// Live file copies (path, size) for each content hash, in input order.
    /// Backs the commit flow (§9.7). Unknown hashes yield empty lists.
    #[allow(clippy::type_complexity)]
    pub fn files_for_hashes(&self, hashes: &[String]) -> Result<Vec<(String, Vec<(String, u64)>)>> {
        let mut out = Vec::with_capacity(hashes.len());
        for hex in hashes {
            let bytes: Vec<u8> = (0..hex.len().saturating_sub(1))
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
                .collect();
            let files = if bytes.len() == 32 {
                self.db.files_for_hash(&bytes)?
            } else {
                Vec::new()
            };
            out.push((hex.clone(), files));
        }
        Ok(out)
    }

    /// Overrides the §7 scoring weights used by cluster passes.
    pub fn set_quality_weights(&mut self, weights: cluster::scoring::QualityWeights) {
        self.quality_weights = weights;
    }
}

/// A stored cluster of near-duplicate images.
#[derive(Debug, Clone)]
pub struct NearCluster {
    pub id: i64,
    /// Member paths, sorted.
    pub files: Vec<String>,
}

/// One member of a stored cluster, with what the review UIs need.
#[derive(Debug, Clone)]
pub struct ClusterMember {
    pub file_id: i64,
    pub path: String,
    pub thumb_path: Option<String>,
    pub quality_score: Option<f64>,
    pub content_hash_hex: String,
    pub captured_at: Option<i64>,
}

/// A stored cluster of any kind, members in filmstrip order (capture time
/// ascending, then id).
#[derive(Debug, Clone)]
pub struct ClusterDetail {
    pub id: i64,
    pub kind: String,
    pub keeper_file_id: Option<i64>,
    pub members: Vec<ClusterMember>,
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
    /// BLAKE3 hex; the key for triage decisions (PRD §6).
    pub content_hash_hex: String,
}

/// One event's winner for the best-of view (§9.5).
#[derive(Debug, Clone)]
pub struct BestOfEntry {
    /// Event bounds, unix seconds.
    pub start: i64,
    pub end: i64,
    /// Photos in the event.
    pub count: u64,
    /// Highest quality (then aesthetic, then sharpness) member.
    pub item: GridItem,
}
