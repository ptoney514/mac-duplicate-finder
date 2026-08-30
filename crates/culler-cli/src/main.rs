//! Thin CLI over culler-core for validating the engine before the UI exists.
//! Argument parsing is hand-rolled: clap is not on the PRD's dependency list.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use culler_core::cluster::near::{DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX};
use culler_core::{DupeGroup, Engine, ScanProgress};

const USAGE: &str = "\
usage: culler-cli [--db <path>] <command>

commands:
  scan <path>   walk a folder, record file facts, hash and analyze images
  dupes         list exact-duplicate groups, largest reclaimable first
  clusters      rebuild and list near-duplicate clusters
                  [--kind near] [--dhash <max>] [--phash <max>]

options:
  --db <path>   database file (default: ~/Library/Application Support/Culler/culler.db)";

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

    let result = match args.first().map(String::as_str) {
        Some("scan") => match args.get(1) {
            Some(root) if args.len() == 2 => cmd_scan(&db_path, root),
            _ => return usage_error("scan takes exactly one path"),
        },
        Some("dupes") if args.len() == 1 => cmd_dupes(&db_path),
        Some("dupes") => return usage_error("dupes takes no arguments"),
        Some("clusters") => match parse_cluster_args(&args[1..]) {
            Ok((dhash_max, phash_max)) => cmd_clusters(&db_path, dhash_max, phash_max),
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
    match args.iter().position(|a| a == "--db") {
        None => Ok(None),
        Some(i) if i + 1 < args.len() => {
            args.remove(i);
            Ok(Some(PathBuf::from(args.remove(i))))
        }
        Some(_) => Err("--db needs a path".to_owned()),
    }
}

fn default_db_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Culler/culler.db"))
}

fn cmd_scan(db_path: &Path, root: &str) -> Result<(), culler_core::CoreError> {
    let mut engine = Engine::open(db_path)?;
    let started = Instant::now();
    let summary = engine.scan(std::path::Path::new(root), &mut |progress| match progress {
        ScanProgress::Walking { found } => eprint!("\r  walking… {found} images found"),
        ScanProgress::Hashing { done, total } => eprint!("\r  hashing… {done}/{total} files"),
        ScanProgress::Analyzing { done, total } => {
            eprint!("\r  analyzing… {done}/{total} images")
        }
    })?;
    eprint!("\r\x1b[2K");

    println!("scanned {root} in {:.1}s", started.elapsed().as_secs_f64());
    println!(
        "  found {} images ({} new, {} changed, {} unchanged, {} missing)",
        summary.found, summary.added, summary.updated, summary.unchanged, summary.missing
    );
    println!(
        "  hashed {} files, analyzed {} images ({} errors)",
        summary.hashed, summary.analyzed, summary.errors
    );
    Ok(())
}

/// `clusters [--kind near] [--dhash <max>] [--phash <max>]`
fn parse_cluster_args(args: &[String]) -> Result<(u32, u32), String> {
    let (mut dhash_max, mut phash_max) = (DEFAULT_DHASH_MAX, DEFAULT_PHASH_MAX);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .ok_or_else(|| format!("{name} needs a value"))
                .and_then(|v| {
                    v.parse::<u32>()
                        .map_err(|_| format!("{name}: not a number: {v}"))
                })
        };
        match flag.as_str() {
            "--kind" => match it.next().map(String::as_str) {
                Some("near") => {}
                Some(other) => {
                    return Err(format!(
                        "unsupported cluster kind: {other} (only 'near' exists; \
                         burst arrives with embeddings in milestone 5)"
                    ))
                }
                None => return Err("--kind needs a value".to_owned()),
            },
            "--dhash" => dhash_max = value("--dhash")?,
            "--phash" => phash_max = value("--phash")?,
            other => return Err(format!("unknown clusters flag: {other}")),
        }
    }
    if dhash_max > 64 || phash_max > 64 {
        return Err("hash distances are at most 64".to_owned());
    }
    Ok((dhash_max, phash_max))
}

fn cmd_clusters(
    db_path: &Path,
    dhash_max: u32,
    phash_max: u32,
) -> Result<(), culler_core::CoreError> {
    let mut engine = Engine::open(db_path)?;
    let clusters = engine.cluster_near(dhash_max, phash_max)?;

    if clusters.is_empty() {
        println!("no near-duplicate clusters (dhash ≤ {dhash_max}, phash ≤ {phash_max})");
        return Ok(());
    }

    let members: usize = clusters.iter().map(|c| c.files.len()).sum();
    println!(
        "{} near-duplicate cluster{} covering {} images (dhash ≤ {dhash_max}, phash ≤ {phash_max})",
        clusters.len(),
        plural(clusters.len()),
        members
    );
    for cluster in &clusters {
        println!();
        println!("cluster {}: {} images", cluster.id, cluster.files.len());
        for file in &cluster.files {
            println!("  {file}");
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
