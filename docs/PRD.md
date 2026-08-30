# PRD: Photo Library Curator (working name: Culler)

## 1. Purpose

A native macOS app for one user with a personal library of several hundred thousand photos. It finds exact and near-duplicate images, groups bursts and similar shots, scores image quality, picks the best of each group, and lets the user search the library by meaning. Local models do the bulk of the work. A frontier model (Claude API) is used sparingly for final judgment calls.

This is not a general Mac cleaner and it does not handle video in v1.

## 2. Goals

- Index hundreds of thousands of images without the UI ever blocking.
- Find exact duplicates with zero false positives.
- Cluster near-duplicates and bursts with tunable sensitivity.
- Rank images within a cluster and propose a keeper.
- Search the library with natural language ("kids at the beach 2019").
- Let the user triage fast with keyboard-driven flows.
- Never destroy data without an explicit commit step.

## 3. Non-goals (v1)

- Video files of any kind.
- System cache, log, or app cleanup.
- Cloud sync, iCloud Photos library integration, or Photos.app database editing.
- Multi-user or App Store distribution.
- Editing images.

## 4. User

Single user, the developer. Comfortable with a CLI. Runs the app on a Mac mini with the library on local or attached storage. Wants a tool that is fast, honest about confidence, and pleasant to use for long triage sessions.

## 5. Architecture

Three layers with a hard boundary between them.

### 5.1 Rust engine (`culler-core`)

A Rust crate that owns all file scanning, hashing, image analysis, embeddings, and persistence. It exposes a narrow API to Swift through UniFFI. It has no UI and no knowledge of SwiftUI.

Responsibilities:
- Directory walking with include/exclude rules.
- Content hashing (BLAKE3) with size-first candidate grouping and a first-4KB prehash before full hashing.
- EXIF extraction (capture time, camera, dimensions, orientation).
- Thumbnail generation (256px long edge, WebP or JPEG) cached on disk, keyed by content hash.
- Perceptual hashing (dHash and pHash, 64-bit each).
- CLIP image embeddings (512-dim) via ONNX Runtime.
- Nearest-neighbor index over embeddings (usearch).
- Quality signals per image (see section 7).
- Clustering of near-duplicates and bursts.
- SQLite persistence in WAL mode. This is the source of truth for file facts.
- Incremental rescans: skip files whose path, size, and mtime are unchanged.
- Progress reporting via a callback interface so Swift can show live status.

Crates to prefer: `walkdir`, `blake3`, `rusqlite` (bundled), `image`, `kamadak-exif`, `ort` (ONNX Runtime), `usearch`, `rayon`, `uniffi`, `thiserror`, `tracing`.

### 5.2 Swift shell (`Culler.app`)

SwiftUI macOS app targeting macOS 14+. Talks only to the UniFFI-generated Swift bindings. SwiftData is used only for user decisions and UI state (kept, rejected, maybe, tags, session history). It never stores file facts that the Rust DB already holds.

Responsibilities:
- Library selection and scan control.
- Grid, cluster, and triage views backed by a lazy thumbnail loader.
- Keyboard-driven triage.
- Search bar wired to the engine's semantic search.
- Review and commit flow for deletions.
- Settings for sensitivity, model paths, and API keys.

### 5.3 AI tier

Two levels, both optional at runtime.

Local (default, always available offline):
- CLIP ViT-B/32 ONNX for embeddings and text search.
- An aesthetic scoring model in ONNX (LAION aesthetic predictor on top of CLIP embeddings is the simplest choice since it reuses the same embedding).
- Apple Vision framework from the Swift side for face detection and face landmark checks (eyes open), because it ships with the OS and is fast.

Frontier (opt-in, user supplies a key):
- Claude API used only for cluster tiebreaks. Send the top N thumbnails from one cluster plus their quality signals and ask for a ranked pick with a one-line reason. Cache the result by cluster hash so a cluster is never sent twice. Never send the full library.

## 6. Data model (SQLite, owned by Rust)

```
files
  id INTEGER PK
  path TEXT UNIQUE
  size INTEGER
  mtime INTEGER
  content_hash BLOB        -- BLAKE3, null until hashed
  status TEXT              -- pending | hashed | analyzed | missing

images
  file_id INTEGER PK FK files
  width INTEGER
  height INTEGER
  captured_at INTEGER      -- unix time from EXIF, nullable
  camera TEXT
  orientation INTEGER
  dhash INTEGER
  phash INTEGER
  sharpness REAL
  exposure_score REAL
  face_count INTEGER
  eyes_open_ratio REAL
  aesthetic_score REAL
  quality_score REAL       -- composite, see section 7
  thumb_path TEXT

embeddings
  file_id INTEGER PK FK images
  vector BLOB              -- 512 x f32

clusters
  id INTEGER PK
  kind TEXT                -- exact | near | burst
  keeper_file_id INTEGER   -- engine's proposed keeper
  created_at INTEGER

cluster_members
  cluster_id INTEGER FK
  file_id INTEGER FK
  rank INTEGER
  PRIMARY KEY (cluster_id, file_id)

frontier_verdicts
  cluster_id INTEGER PK
  model TEXT
  keeper_file_id INTEGER
  reason TEXT
  created_at INTEGER
```

User decisions (keep / reject / maybe / tag) live in SwiftData on the Swift side, keyed by `content_hash` so they survive file moves.

## 7. Quality scoring

Each image gets a composite `quality_score` in [0, 1] from these signals:

- Sharpness: variance of Laplacian on a grayscale downscale. Normalized within the cluster, not globally.
- Exposure: histogram clipping penalty for blown highlights and crushed shadows.
- Faces: count and eyes-open ratio from Vision. Shots with faces where eyes are open score higher when the cluster contains faces.
- Aesthetic: LAION predictor output on the CLIP embedding.
- Resolution: mild bonus for the largest image in a cluster.

Weights are configurable. Default: sharpness 0.30, faces 0.25, aesthetic 0.25, exposure 0.15, resolution 0.05. The keeper is the highest composite score unless the frontier tier overrides it.

## 8. Clustering rules

- Exact: identical `content_hash`.
- Near: dHash Hamming distance <= threshold (default 8) OR pHash distance <= threshold (default 10). Threshold exposed in settings.
- Burst: captured within 3 seconds of each other on the same camera AND embedding cosine similarity >= 0.92. Burst threshold exposed in settings.
- Embedding-only similarity (cosine >= 0.95 with no hash match) is surfaced as a "possible" cluster and shown separately with lower confidence.

Clustering runs as a pass after analysis and can be re-run with new thresholds without re-analyzing.

## 9. Core user flows

### 9.1 First scan
User picks one or more root folders. Engine walks, hashes, analyzes, and embeds in a background pipeline with rayon. UI shows a live counter (found / hashed / analyzed / embedded) and stays responsive. Scan is resumable if the app quits.

### 9.2 Duplicate review
List of exact-duplicate clusters sorted by reclaimable bytes. Each cluster shows one thumbnail and the file paths. Default action keeps the copy with the oldest mtime in the shortest path. User can override. Batch "accept all defaults" available.

### 9.3 Burst resolver
Filmstrip view of a near or burst cluster. Keeper is starred. Arrow keys move, Enter accepts the keeper, K sets a different keeper, R rejects all but the keeper, M marks maybe. Space toggles full-screen compare of the two highest-scored frames. Optional "Ask Claude" button sends the cluster for a frontier verdict and shows the reason inline.

### 9.4 Semantic search
Search field. Text is embedded with the CLIP text encoder and queried against usearch. Results appear as a grid within 100 ms for a 300k library. Filters for date range and camera.

### 9.5 Best-of view
"Top N per period" view. Groups the library by day or event (gap-based clustering on `captured_at`, gap = 2 hours), then shows the highest `quality_score` image per group. Useful for "year in 50 photos."

### 9.6 Triage mode
Full-screen single image, keyboard-only. J/K or arrows to move, 1 keep, 2 maybe, 3 reject. Decisions write to SwiftData immediately. Undo with Z.

### 9.7 Commit
Review screen listing every file marked reject with size totals. User confirms. Files are moved to a staging folder `~/Culler Staging/<timestamp>/` with a manifest JSON mapping original paths. A separate "empty staging" action moves them to the macOS Trash using `FileManager.trashItem`. Nothing is ever deleted directly.

## 10. Performance targets

- Hashing: >= 500 MB/s on SSD, limited by disk not CPU.
- Analysis + embedding: >= 40 images/sec on Apple Silicon with ONNX Runtime CoreML EP.
- Full first scan of 300k images completes in under 3 hours.
- Grid scroll stays at 60 fps with thumbnails loaded lazily from the cache.
- Semantic query returns in under 100 ms.
- App memory under 2 GB during a scan.

## 11. Repository layout

```
culler/
  Cargo.toml                 # workspace
  crates/
    culler-core/             # engine, UniFFI exported
      src/
        lib.rs
        scan/                # walker, hashing, incremental logic
        analyze/             # exif, thumbs, phash, quality signals
        embed/               # onnx session, clip, aesthetic
        index/               # usearch wrapper
        cluster/             # exact, near, burst, gap clustering
        db/                  # rusqlite schema, migrations, queries
        api.rs               # uniffi interface surface
      uniffi.toml
    culler-cli/              # thin CLI over core for testing and batch runs
  apple/
    Culler/                  # Xcode project, SwiftUI app
      Culler/
        App/
        Views/
        Models/              # SwiftData models for decisions
        Engine/              # generated bindings + Swift wrapper
        Vision/              # face and eyes-open via Apple Vision
        Frontier/            # Claude API client
      CullerTests/
  models/                    # downloaded ONNX files, gitignored
  scripts/
    build-xcframework.sh     # cargo build for arm64 + x86_64, package for Xcode
    fetch-models.sh
  docs/
    PRD.md                   # this file
    ADRs/
```

## 12. Build and tooling

- Rust stable, edition 2021. `cargo build --release` for both `aarch64-apple-darwin` and `x86_64-apple-darwin`, combined into an XCFramework by the build script.
- UniFFI generates Swift bindings at build time. Commit the generated Swift to the repo so Xcode builds without Rust installed.
- Xcode 16+, Swift 6 strict concurrency.
- ONNX models fetched by script, never committed.
- `culler-cli` supports `scan <path>`, `dupes`, `clusters --kind near`, `search "<text>"`, and `stats` so the engine can be validated before the UI exists.
- Tests: Rust unit tests for hashing, clustering thresholds, and DB migrations. A fixture folder with ~200 real images (duplicates, bursts, blurry, sharp) for integration tests.

## 13. Milestones

1. Engine scaffold, walker, BLAKE3 hashing, SQLite, CLI `scan` and `dupes`. Exact duplicates working end to end from the CLI.
2. EXIF, thumbnails, dHash/pHash, near clustering. CLI `clusters`.
3. Xcode project, UniFFI bindings, XCFramework build. Grid view showing thumbnails. Duplicate review flow.
4. CLIP embeddings via ONNX, usearch index, semantic search in CLI and UI.
5. Quality signals, aesthetic model, Vision face checks, burst resolver UI.
6. Triage mode, best-of view, staging and commit flow.
7. Frontier tiebreak with caching.

## 14. Open questions

- CLIP ViT-B/32 vs a larger model. Start small, measure search quality, upgrade if needed.
- Whether `ort` with the CoreML execution provider is stable enough on the target Xcode toolchain, or whether to fall back to CPU EP.
- Thumbnail format: WebP is smaller, JPEG has zero dependency risk. Decide during milestone 2.
- Whether to keep user decisions in SwiftData or move them into the Rust DB for simplicity. Leaning SwiftData for now.

## 15. Instructions for Claude Code

Read this document fully before writing code. Scaffold the repository layout in section 11. Start with milestone 1 only. Do not build UI until the CLI proves the engine works on a real folder. Write an ADR in `docs/ADRs/` for any deviation from this document. Ask before adding a dependency not listed in section 5.1. Prefer boring, well-maintained crates over clever ones. Keep the UniFFI surface small and stable; add functions only when the Swift side needs them.
