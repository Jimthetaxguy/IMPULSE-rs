use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use crate::{build_hygiene, state, verify};

use super::{load_build_hygiene_config, print_json, print_verification_report};

/// Handle the `verify` command.
///
/// Runs the default verification steps (build, test, clippy, fmt) and prints
/// a pass/fail report. Bails on failure.
pub fn handle_verify() -> Result<()> {
    let steps = verify::default_steps(&std::env::current_dir()?);
    let report = verify::run_verification(steps)?;
    print_verification_report(&report);
    if !report.success() {
        anyhow::bail!("Verification failed");
    }
    Ok(())
}

/// Handle the `clean-all` command.
///
/// Runs `cargo clean` across all discovered Rust projects under the
/// configured scan paths. Respects the dry-run flag.
pub fn handle_clean_all(state: &Arc<state::State>, dry_run: Option<bool>) -> Result<()> {
    let config = load_build_hygiene_config(state);
    let dry_run = dry_run.unwrap_or(config.dry_run_default);
    let paths = config.expanded_scan_paths();

    if paths.is_empty() {
        println!("No scan paths configured.");
        return Ok(());
    }

    println!("=== Cargo Clean All ===\n");
    println!(
        "Mode: {}\n",
        if dry_run {
            "DRY RUN"
        } else {
            "LIVE \u{2014} will cargo clean all projects!"
        }
    );

    match build_hygiene::clean_all::clean_all_projects(&paths, dry_run) {
        Ok(result) => {
            println!("{}", result.summary);
            if !result.errors.is_empty() {
                println!("\nWarnings:");
                for err in &result.errors {
                    println!("  - {}", err);
                }
            }
        }
        Err(e) => {
            eprintln!("Clean-all failed: {}", e);
        }
    }
    Ok(())
}

/// Handle the `sccache-setup` command.
///
/// When `check` or `json` is set, reports sccache installation and cache
/// stats. Otherwise, performs initial sccache setup and configuration.
pub fn handle_sccache_setup(check: bool, json: bool) -> Result<()> {
    if check || json {
        let status = build_hygiene::sccache::sccache_status();
        if json {
            print_json(&status)?;
        } else {
            println!("=== sccache Status ===\n");
            println!("Installed: {}", if status.installed { "yes" } else { "no" });
            if let Some(ref v) = status.version {
                println!("Version: {}", v);
            }
            println!(
                "Configured: {}",
                if status.configured { "yes" } else { "no" }
            );
            println!("Config path: {}", status.config_path);
            if let Some(ref stats) = status.stats {
                println!("\nCache Stats:");
                if let Some(hits) = stats.cache_hits {
                    println!("  Hits: {}", hits);
                }
                if let Some(misses) = stats.cache_misses {
                    println!("  Misses: {}", misses);
                }
                if let Some(ref size) = stats.cache_size {
                    println!("  Size: {}", size);
                }
            }
        }
    } else {
        match build_hygiene::sccache::sccache_setup(false) {
            Ok(result) => {
                println!("=== sccache Setup ===\n");
                println!("{}", result.action_taken);
                println!("Config: {}", result.config_path);
            }
            Err(e) => {
                eprintln!("sccache setup failed: {}", e);
            }
        }
    }
    Ok(())
}

/// Handle the `build-health` command.
///
/// Discovers Rust projects under the configured scan paths, measures
/// `target/` directory sizes, and prints a disk-usage report with
/// recommendations and sccache status.
pub fn handle_build_health(state: &Arc<state::State>, json: bool) -> Result<()> {
    let config = load_build_hygiene_config(state);
    let paths = config.expanded_scan_paths();

    let projects = build_hygiene::discovery::discover_rust_projects(&paths);
    let report = build_hygiene::measurement::generate_report(&projects, config.size_threshold_gb);

    if json {
        print_json(&report)?;
    } else {
        println!("=== Rust Build Health ===\n");
        println!(
            "Total: {} across {} projects\n",
            report.total_human, report.project_count
        );

        if !report.projects.is_empty() {
            println!("Projects (largest first):");
            for (i, p) in report.projects.iter().enumerate().take(20) {
                println!("  {}. {} \u{2014} {}", i + 1, p.path, p.target_size_human);
            }
            if report.projects.len() > 20 {
                println!("  ... and {} more", report.projects.len() - 20);
            }
        }

        println!("\nRecommendations:");
        for rec in &report.recommendations {
            println!("  - {}", rec);
        }

        let sccache_st = build_hygiene::sccache::sccache_status();
        println!(
            "\nsccache: {}",
            if sccache_st.installed && sccache_st.configured {
                "configured"
            } else if sccache_st.installed {
                "installed but not configured \u{2014} run `impulse-rs sccache-setup`"
            } else {
                "not installed \u{2014} run `cargo install sccache`"
            }
        );
    }
    Ok(())
}

/// Handle the `sweep` command.
///
/// Removes stale Rust build artifacts older than the specified number of
/// days. Scans the given path or the configured scan paths.
pub fn handle_sweep(
    state: &Arc<state::State>,
    dry_run: Option<bool>,
    path: Option<PathBuf>,
    days: Option<u32>,
    verbose: bool,
) -> Result<()> {
    let config = load_build_hygiene_config(state);
    let dry_run = dry_run.unwrap_or(config.dry_run_default);
    let days = days.unwrap_or(config.age_threshold_days);

    let paths = if let Some(p) = path {
        vec![p]
    } else {
        config.expanded_scan_paths()
    };

    if paths.is_empty() {
        println!("No scan paths configured. Use --path or set build_hygiene_scan_paths in config.");
        return Ok(());
    }

    println!("=== Cargo Sweep ===\n");
    println!(
        "Scanning: {:?}",
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    );
    println!("Artifacts older than: {} days", days);
    println!("Mode: {}\n", if dry_run { "DRY RUN" } else { "LIVE" });

    let opts = build_hygiene::sweep::SweepOptions {
        days,
        dry_run,
        paths,
        recursive: true,
        verbose,
    };

    match build_hygiene::sweep::run_sweep(&opts) {
        Ok(result) => {
            println!("{}", result.summary);
            if !result.errors.is_empty() {
                println!("\nWarnings:");
                for err in &result.errors {
                    println!("  - {}", err);
                }
            }
        }
        Err(e) => {
            eprintln!("Sweep failed: {}", e);
            eprintln!("\nHint: Check filesystem permissions and scan path configuration.");
        }
    }
    Ok(())
}

/// Handle the `wipe` command.
///
/// Aggressively deletes entire `target/` directories under the given path
/// or configured scan paths. Respects the dry-run flag.
pub fn handle_wipe(
    state: &Arc<state::State>,
    dry_run: Option<bool>,
    path: Option<PathBuf>,
) -> Result<()> {
    let config = load_build_hygiene_config(state);
    let dry_run = dry_run.unwrap_or(config.dry_run_default);

    let paths = if let Some(p) = path {
        vec![p]
    } else {
        config.expanded_scan_paths()
    };

    if paths.is_empty() {
        println!("No scan paths configured. Use --path or set build_hygiene_scan_paths in config.");
        return Ok(());
    }

    println!("=== Cargo Wipe ===\n");
    println!(
        "Mode: {}\n",
        if dry_run {
            "DRY RUN (safe)"
        } else {
            "LIVE \u{2014} will delete target/ dirs!"
        }
    );

    let opts = build_hygiene::wipe::WipeOptions {
        dry_run,
        paths,
        include_node_modules: false,
    };

    match build_hygiene::wipe::run_wipe(&opts) {
        Ok(result) => {
            println!("{}", result.summary);
            if !result.errors.is_empty() {
                println!("\nWarnings:");
                for err in &result.errors {
                    println!("  - {}", err);
                }
            }
        }
        Err(e) => {
            eprintln!("Wipe failed: {}", e);
            eprintln!("\nHint: Check filesystem permissions and scan path configuration.");
        }
    }
    Ok(())
}
