//! UniFFI interface surface: the only thing the Swift shell sees. Keep it
//! small and stable; add functions only when the Swift side needs them
//! (PRD section 15). Types are mirrored as plain records so core types stay
//! free of binding concerns; content hashes cross the boundary as hex.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::{CoreError, Engine};

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum ApiError {
    #[error("{message}")]
    Engine { message: String },
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        Self::Engine {
            message: e.to_string(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ScanSummary {
    pub found: u64,
    pub added: u64,
    pub updated: u64,
    pub unchanged: u64,
    pub missing: u64,
    pub hashed: u64,
    pub analyzed: u64,
    pub embedded: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum ScanProgress {
    Walking { found: u64 },
    Hashing { done: u64, total: u64 },
    Analyzing { done: u64, total: u64 },
    Embedding { done: u64, total: u64 },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DupeFile {
    pub path: String,
    pub mtime: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DupeGroup {
    /// BLAKE3 content hash, lowercase hex. Stable key for user decisions.
    pub hash_hex: String,
    pub size: u64,
    /// Keeper-first (oldest mtime, then shortest path, then path).
    pub files: Vec<DupeFile>,
    pub reclaimable: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct NearCluster {
    pub id: i64,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LibraryItem {
    pub file_id: i64,
    pub path: String,
    pub thumb_path: Option<String>,
    pub captured_at: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ClusterMember {
    pub file_id: i64,
    pub path: String,
    pub thumb_path: Option<String>,
    pub quality_score: Option<f64>,
    pub content_hash_hex: String,
    pub captured_at: Option<i64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ClusterDetail {
    pub id: i64,
    pub kind: String,
    pub keeper_file_id: Option<i64>,
    /// Filmstrip order: capture time ascending, then id.
    pub members: Vec<ClusterMember>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FaceTarget {
    pub file_id: i64,
    pub thumb_path: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FaceFacts {
    pub file_id: i64,
    pub face_count: u32,
    pub eyes_open_ratio: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchResult {
    pub file_id: i64,
    pub path: String,
    pub thumb_path: Option<String>,
    /// Cosine similarity in [-1, 1]; higher is better.
    pub score: f32,
}

/// Swift implements this to receive live scan status (PRD section 5.1).
#[uniffi::export(with_foreign)]
pub trait ScanProgressListener: Send + Sync {
    fn on_progress(&self, progress: ScanProgress);
}

/// The engine handle Swift holds. Methods serialize on an internal lock;
/// long operations (scan) should be called off the main actor.
#[derive(uniffi::Object)]
pub struct CullerEngine {
    inner: Mutex<Engine>,
}

#[uniffi::export]
impl CullerEngine {
    /// Opens (creating if needed) the database and runs migrations.
    #[uniffi::constructor]
    pub fn open(db_path: String) -> Result<Arc<Self>, ApiError> {
        Ok(Arc::new(Self {
            inner: Mutex::new(Engine::open(Path::new(&db_path))?),
        }))
    }

    /// Walks, records, hashes, and analyzes `root`. Blocking.
    pub fn scan(
        &self,
        root: String,
        listener: Arc<dyn ScanProgressListener>,
    ) -> Result<ScanSummary, ApiError> {
        let summary = self.lock().scan(Path::new(&root), &mut |progress| {
            listener.on_progress(match progress {
                crate::ScanProgress::Walking { found } => ScanProgress::Walking { found },
                crate::ScanProgress::Hashing { done, total } => {
                    ScanProgress::Hashing { done, total }
                }
                crate::ScanProgress::Analyzing { done, total } => {
                    ScanProgress::Analyzing { done, total }
                }
                crate::ScanProgress::Embedding { done, total } => {
                    ScanProgress::Embedding { done, total }
                }
            });
        })?;
        Ok(ScanSummary {
            found: summary.found,
            added: summary.added,
            updated: summary.updated,
            unchanged: summary.unchanged,
            missing: summary.missing,
            hashed: summary.hashed,
            analyzed: summary.analyzed,
            embedded: summary.embedded,
            errors: summary.errors,
        })
    }

    /// Exact-duplicate groups, largest reclaimable first.
    pub fn dupes(&self) -> Result<Vec<DupeGroup>, ApiError> {
        Ok(self
            .lock()
            .dupes()?
            .into_iter()
            .map(|g| DupeGroup {
                hash_hex: g.hash.iter().map(|b| format!("{b:02x}")).collect(),
                size: g.size,
                files: g
                    .files
                    .into_iter()
                    .map(|f| DupeFile {
                        path: f.path,
                        mtime: f.mtime,
                    })
                    .collect(),
                reclaimable: g.reclaimable,
            })
            .collect())
    }

    /// Rebuilds and returns near-duplicate clusters (no re-analysis).
    pub fn cluster_near(
        &self,
        dhash_max: u32,
        phash_max: u32,
    ) -> Result<Vec<NearCluster>, ApiError> {
        Ok(self
            .lock()
            .cluster_near(dhash_max, phash_max)?
            .into_iter()
            .map(|c| NearCluster {
                id: c.id,
                files: c.files,
            })
            .collect())
    }

    /// Rebuilds burst clusters (PRD §8) and refreshes quality + keepers.
    pub fn cluster_bursts(
        &self,
        max_gap_secs: i64,
        min_cosine: f32,
    ) -> Result<Vec<NearCluster>, ApiError> {
        Ok(self
            .lock()
            .cluster_bursts(max_gap_secs, min_cosine)?
            .into_iter()
            .map(|c| NearCluster {
                id: c.id,
                files: c.files,
            })
            .collect())
    }

    /// Stored clusters with member metadata; `kind` filters (near/burst).
    pub fn clusters(&self, kind: Option<String>) -> Result<Vec<ClusterDetail>, ApiError> {
        Ok(self
            .lock()
            .clusters(kind.as_deref())?
            .into_iter()
            .map(|c| ClusterDetail {
                id: c.id,
                kind: c.kind,
                keeper_file_id: c.keeper_file_id,
                members: c
                    .members
                    .into_iter()
                    .map(|m| ClusterMember {
                        file_id: m.file_id,
                        path: m.path,
                        thumb_path: m.thumb_path,
                        quality_score: m.quality_score,
                        content_hash_hex: m.content_hash_hex,
                        captured_at: m.captured_at,
                    })
                    .collect(),
            })
            .collect())
    }

    /// Analyzed images the Vision face pass hasn't visited yet.
    pub fn images_needing_faces(&self) -> Result<Vec<FaceTarget>, ApiError> {
        Ok(self
            .lock()
            .images_needing_faces()?
            .into_iter()
            .map(|(file_id, thumb_path)| FaceTarget {
                file_id,
                thumb_path,
            })
            .collect())
    }

    /// Stores Apple Vision face facts and refreshes quality + keepers.
    pub fn store_face_facts(&self, facts: Vec<FaceFacts>) -> Result<(), ApiError> {
        let rows: Vec<(i64, u32, f64)> = facts
            .into_iter()
            .map(|f| (f.file_id, f.face_count, f.eyes_open_ratio))
            .collect();
        Ok(self.lock().store_face_facts(&rows)?)
    }

    /// Loads the CLIP ONNX models so scans embed and `search` works.
    pub fn attach_models(&self, models_dir: String) -> Result<(), ApiError> {
        Ok(self.lock().attach_models(Path::new(&models_dir))?)
    }

    /// Whether models are attached (search available).
    pub fn has_models(&self) -> bool {
        self.lock().has_embedder()
    }

    /// Semantic search over the library, best match first.
    pub fn search(&self, query: String, limit: u32) -> Result<Vec<SearchResult>, ApiError> {
        Ok(self
            .lock()
            .search(&query, limit as usize)?
            .into_iter()
            .map(|r| SearchResult {
                file_id: r.file_id,
                path: r.path,
                thumb_path: r.thumb_path,
                score: r.score,
            })
            .collect())
    }

    /// Library grid page: analyzed images, newest capture first.
    pub fn grid_items(&self, offset: u64, limit: u64) -> Result<Vec<LibraryItem>, ApiError> {
        Ok(self
            .lock()
            .grid_items(offset, limit)?
            .into_iter()
            .map(|i| LibraryItem {
                file_id: i.file_id,
                path: i.path,
                thumb_path: i.thumb_path,
                captured_at: i.captured_at,
                width: i.width,
                height: i.height,
            })
            .collect())
    }
}

impl CullerEngine {
    fn lock(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
