//! Thin CLI over culler-core for validating the engine before the UI exists.
//! Argument parsing is hand-rolled: clap is not on the PRD's dependency list.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use culler_core::cluster::burst::{DEFAULT_BURST_GAP_SECS, DEFAULT_BURST_MIN_COSINE};
use culler_core::cluster::near::{DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX};
use culler_core::{DupeGroup, Engine, ScanProgress};

const USAGE: &str = "\
usage: culler-cli [--db <path>] <command>

commands:
  scan <path>       walk a folder, record file facts, hash, analyze, and
                    (when models are installed) embed images
  dupes             list exact-duplicate groups, largest reclaimable first
  clusters          rebuild and list near or burst clusters
                      [--kind near|burst] [--dhash <n>] [--phash <n>]
                      [--gap <secs>] [--cos <min>]
  search \"<text>\"   semantic search (needs scripts/fetch-models.sh once)
                      [--limit <n>]

options:
  --db <path>       database file (default: ~/Library/Application Support/Culler/culler.db)
  --models <path>   CLIP model dir (default: ~/Library/Application Support/Culler/models)";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    let db_path = match take_db_flag(&mut args) {
        Ok(Some(p)) => p,
        Ok(None) => match default_db_path() {
            Some(p) => p,
            None => {
                eprintln!("error: HOME is not set; pass --db <path>");
                return ExitCode::from(1);
            }
        },
        Err(msg) => return usage_error(&msg),
    };

    let models_dir = match take_path_flag(&mut args, "--models") {
        Ok(Some(p)) => p,
        Ok(None) => EngineStorePaths::default_models(),
        Err(msg) => return usage_error(&msg),
    };

    let result = match args.first().map(String::as_str) {
        Some("scan") => match args.get(1) {
            Some(root) if args.len() == 2 => cmd_scan(&db_path, &models_dir, root),
            _ => return usage_error("scan takes exactly one path"),
        },
        Some("search") => match args.get(1) {
            Some(query) => match parse_limit(&args[2..]) {
                Ok(limit) => cmd_search(&db_path, &models_dir, query, limit),
                Err(msg) => return usage_error(&msg),
            },
            None => return usage_error("search takes a quoted query"),
        },
        Some("dupes") if args.len() == 1 => cmd_dupes(&db_path),
        Some("dupes") => return usage_error("dupes takes no arguments"),
        Some("clusters") => match parse_cluster_args(&args[1..]) {
            Ok(run) => cmd_clusters(&db_path, run),
            Err(msg) => return usage_error(&msg),
        },
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => return usage_error(&format!("unknown command: {other}")),
        None => return usage_error("no command given"),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

fn take_db_flag(args: &mut Vec<String>) -> Result<Option<PathBuf>, String> {
    take_path_flag(args, "--db")
}

fn take_path_flag(args: &mut Vec<String>, flag: &str) -> Result<Option<PathBuf>, String> {
    match args.iter().position(|a| a == flag) {
        None => Ok(None),
        Some(i) if i + 1 < args.len() => {
            args.remove(i);
            Ok(Some(PathBuf::from(args.remove(i))))
        }
        Some(_) => Err(format!("{flag} needs a path")),
    }
}

fn default_db_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Culler/culler.db"))
}

/// Default locations shared with the app.
struct EngineStorePaths;
impl EngineStorePaths {
    fn default_models() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("Library/Application Support/Culler/models")
    }
}

/// Attaches CLIP models when installed; quiet no-op otherwise.
fn attach_models_if_present(engine: &mut Engine, models_dir: &Path) {
    if models_dir.join("vision_model.onnx").exists() {
        if let Err(e) = engine.attach_models(models_dir) {
            eprintln!(
                "warning: could not load models from {}: {e}",
                models_dir.display()
            );
        }
    }
}

fn parse_limit(args: &[String]) -> Result<usize, String> {
    match args {
        [] => Ok(12),
        [flag, value] if flag == "--limit" => value
            .parse()
            .map_err(|_| format!("--limit: not a number: {value}")),
        _ => Err("search accepts only --limit <n>".to_owned()),
    }
}

fn cmd_search(
    db_path: &Path,
    models_dir: &Path,
    query: &str,
    limit: usize,
) -> Result<(), culler_core::CoreError> {
    let mut engine = Engine::open(db_path)?;
    attach_models_if_present(&mut engine, models_dir);
    let started = Instant::now();
    let hits = engine.search(query, limit)?;
    let elapsed = started.elapsed();

    if hits.is_empty() {
        println!("no results for \"{query}\"");
        return Ok(());
    }
    println!(
        "top {} for \"{query}\" ({} ms)",
        hits.len(),
        elapsed.as_millis()
    );
    for hit in &hits {
        println!("  {:>5.3}  {}", hit.score, hit.path);
    }
    Ok(())
}

fn cmd_scan(db_path: &Path, models_dir: &Path, root: &str) -> Result<(), culler_core::CoreError> {
    let mut engine = Engine::open(db_path)?;
    attach_models_if_present(&mut engine, models_dir);
    let started = Instant::now();
    let summary = engine.scan(std::path::Path::new(root), &mut |progress| match progress {
        ScanProgress::Walking { found } => eprint!("\r  walking… {found} images found"),
        ScanProgress::Hashing { done, total } => eprint!("\r  hashing… {done}/{total} files"),
        ScanProgress::Analyzing { done, total } => {
            eprint!("\r  analyzing… {done}/{total} images")
        }
        ScanProgress::Embedding { done, total } => {
            eprint!("\r  embedding… {done}/{total} images")
        }
    })?;
    eprint!("\r\x1b[2K");

    println!("scanned {root} in {:.1}s", started.elapsed().as_secs_f64());
    println!(
        "  found {} images ({} new, {} changed, {} unchanged, {} missing)",
        summary.found, summary.added, summary.updated, summary.unchanged, summary.missing
    );
    println!(
        "  hashed {} files, analyzed {} images, embedded {} ({} errors)",
        summary.hashed, summary.analyzed, summary.embedded, summary.errors
    );
    Ok(())
}

/// `clusters [--kind near|burst] [--dhash <max>] [--phash <max>]
///           [--gap <secs>] [--cos <min>]`
enum ClusterRun {
    Near { dhash_max: u32, phash_max: u32 },
    Burst { gap_secs: i64, min_cosine: f32 },
}

fn parse_cluster_args(args: &[String]) -> Result<ClusterRun, String> {
    let (mut dhash_max, mut phash_max) = (DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX);
    let (mut gap_secs, mut min_cosine) = (DEFAULT_BURST_GAP_SECS, DEFAULT_BURST_MIN_COSINE);
    let mut kind = "near".to_owned();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut raw = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--kind" => {
                kind = raw("--kind")?;
                if kind != "near" && kind != "burst" {
                    return Err(format!("unsupported cluster kind: {kind} (near or burst)"));
                }
            }
            "--dhash" => {
                dhash_max = raw("--dhash")?
                    .parse()
                    .map_err(|_| "--dhash: not a number".to_owned())?
            }
            "--phash" => {
                phash_max = raw("--phash")?
                    .parse()
                    .map_err(|_| "--phash: not a number".to_owned())?
            }
            "--gap" => {
                gap_secs = raw("--gap")?
                    .parse()
                    .map_err(|_| "--gap: not a number".to_owned())?
            }
            "--cos" => {
                min_cosine = raw("--cos")?
                    .parse()
                    .map_err(|_| "--cos: not a number".to_owned())?
            }
            other => return Err(format!("unknown clusters flag: {other}")),
        }
    }
    if dhash_max > 64 || phash_max > 64 {
        return Err("hash distances are at most 64".to_owned());
    }
    Ok(if kind == "burst" {
        ClusterRun::Burst {
            gap_secs,
            min_cosine,
        }
    } else {
        ClusterRun::Near {
            dhash_max,
            phash_max,
        }
    })
}

fn cmd_clusters(db_path: &Path, run: ClusterRun) -> Result<(), culler_core::CoreError> {
    let mut engine = Engine::open(db_path)?;
    let (kind, description) = match run {
        ClusterRun::Near {
            dhash_max,
            phash_max,
        } => {
            engine.cluster_near(dhash_max, phash_max)?;
            ("near", format!("dhash ≤ {dhash_max}, phash ≤ {phash_max}"))
        }
        ClusterRun::Burst {
            gap_secs,
            min_cosine,
        } => {
            engine.cluster_bursts(gap_secs, min_cosine)?;
            ("burst", format!("gap ≤ {gap_secs}s, cosine ≥ {min_cosine}"))
        }
    };

    let clusters = engine.clusters(Some(kind))?;
    if clusters.is_empty() {
        println!("no {kind} clusters ({description})");
        return Ok(());
    }

    let members: usize = clusters.iter().map(|c| c.members.len()).sum();
    println!(
        "{} {kind} cluster{} covering {} images ({description})",
        clusters.len(),
        plural(clusters.len()),
        members
    );
    for cluster in &clusters {
        println!();
        println!("cluster {}: {} images", cluster.id, cluster.members.len());
        for member in &cluster.members {
            let star = if Some(member.file_id) == cluster.keeper_file_id {
                "★"
            } else {
                " "
            };
            let quality = member
                .quality_score
                .map_or("  -  ".to_owned(), |q| format!("{q:.3}"));
            println!("  {star} {quality}  {}", member.path);
        }
    }
    Ok(())
}

fn cmd_dupes(db_path: &Path) -> Result<(), culler_core::CoreError> {
    let engine = Engine::open(db_path)?;
    let groups = engine.dupes()?;

    if groups.is_empty() {
        println!("no exact duplicates found");
        return Ok(());
    }

    let copies: usize = groups.iter().map(|g| g.files.len() - 1).sum();
    let reclaimable: u64 = groups.iter().map(|g| g.reclaimable).sum();
    println!(
        "{} duplicate group{}, {} redundant cop{}, {} reclaimable",
        groups.len(),
        plural(groups.len()),
        copies,
        if copies == 1 { "y" } else { "ies" },
        human_bytes(reclaimable)
    );

    for (i, group) in groups.iter().enumerate() {
        println!();
        print_group(i + 1, group);
    }
    Ok(())
}

fn print_group(number: usize, group: &DupeGroup) {
    println!(
        "group {number}: {} files × {} each ({} reclaimable) [blake3 {}…]",
        group.files.len(),
        human_bytes(group.size),
        human_bytes(group.reclaimable),
        hex_prefix(&group.hash)
    );
    for (i, file) in group.files.iter().enumerate() {
        let tag = if i == 0 { "keep" } else { "dupe" };
        println!("  {tag}  {}", file.path);
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn hex_prefix(hash: &[u8; 32]) -> String {
    hash[..6].iter().map(|b| format!("{b:02x}")).collect()
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
