# Build Hygiene Module Design

> **Date:** 2026-02-24
> **Status:** Approved
> **Author:** James + Claude
> **Scope:** impulse-rs/src/build_hygiene/

---

## Problem

Rust projects balloon in size due to incremental builds, debug symbols, and multiple toolchain versions. Heavy users can accumulate 100+ GB of build artifacts. There is no automated way in Impulse to detect, report, or clean this waste.

## Solution

Add a **Build Hygiene** module to Impulse that discovers Rust projects, measures disk usage, wraps four cargo cleaning tools, configures sccache for compilation caching, and provides configurable auto-sweep rules via the daemon.

## Architecture

### Module Structure

```
impulse-rs/src/build_hygiene/
├── mod.rs          # Public API, config types, auto-sweep rule engine
├── discovery.rs    # Find Rust projects (target/ dirs) recursively
├── measurement.rs  # Disk usage analysis per project and aggregate
├── sweep.rs        # cargo-sweep wrapper (incremental cleaning)
├── wipe.rs         # cargo-wipe wrapper (aggressive, dry-run-safe)
├── clean_all.rs    # cargo-clean-all wrapper (workspace-wide)
├── sccache.rs      # sccache setup and status checking
└── tests.rs        # Unit + integration tests
```

### Core Types

```rust
/// Configuration for auto-sweep rules, stored in .impulse/config.json
pub struct BuildHygieneConfig {
    pub enabled: bool,
    pub scan_paths: Vec<PathBuf>,
    pub size_threshold_gb: f64,
    pub age_threshold_days: u32,
    pub sweep_on_session_end: bool,
    pub sweep_on_toolchain_update: bool,
    pub dry_run_default: bool,
}

/// Result of scanning for Rust projects
pub struct RustProject {
    pub path: PathBuf,
    pub target_size_bytes: u64,
    pub last_modified: SystemTime,
    pub has_cargo_lock: bool,
    pub toolchain_versions: Vec<String>,
}

/// Result of a sweep/clean operation
pub struct CleanResult {
    pub bytes_freed: u64,
    pub files_removed: u32,
    pub projects_cleaned: u32,
    pub errors: Vec<String>,
    pub was_dry_run: bool,
}
```

### CLI Commands

| Command | Maps to | Behavior |
|---------|---------|----------|
| `impulse-rs sweep [--dry-run] [--path <dir>] [--days <n>]` | cargo-sweep | Incremental clean of stale artifacts, default 30 days |
| `impulse-rs wipe [--dry-run] [--path <dir>]` | cargo-wipe | Aggressive cleaning (dry-run by default for safety) |
| `impulse-rs clean-all [--dry-run]` | cargo-clean-all | Workspace-wide `cargo clean` |
| `impulse-rs sccache-setup [--check]` | sccache config | Write `~/.cargo/config.toml` entry, verify installation |
| `impulse-rs build-health [--json]` | measurement | Disk usage report with recommendations |

### Auto-Sweep Rule Engine

Config in `.impulse/config.json` under `build_hygiene` key:

```json
{
  "build_hygiene": {
    "enabled": true,
    "scan_paths": ["~/projects"],
    "size_threshold_gb": 10.0,
    "age_threshold_days": 30,
    "sweep_on_session_end": true,
    "sweep_on_toolchain_update": true,
    "dry_run_default": true
  }
}
```

Trigger points:
- **Session end**: if `sweep_on_session_end`, run sweep with configured age
- **Health check**: warn when total exceeds `size_threshold_gb`
- **Toolchain update**: compare `rustc --version` against last known version

### Health Integration

New check added to existing `HealthReport`:

```rust
pub fn check_build_health(config: &BuildHygieneConfig) -> HealthCheck {
    let total_gb = measure_total_target_size(&config.scan_paths) as f64 / 1_073_741_824.0;
    if total_gb > config.size_threshold_gb {
        HealthCheck::warning("Rust build artifacts",
            &format!("{:.1} GB exceeds {:.0} GB threshold", total_gb, config.size_threshold_gb))
    } else {
        HealthCheck::healthy("Rust build artifacts")
    }
}
```

### Tool Prerequisite Handling

Each wrapper checks if the cargo extension is installed. If missing, prints install command and offers to install. Tools are added to the `known_tools()` registry in `tools/mod.rs`.

### Error Handling

- All operations return `Result<CleanResult>` via anyhow
- Destructive operations default to dry-run
- sccache setup preserves existing `~/.cargo/config.toml` entries
- File permission errors collected but don't abort

### Testing Strategy

- Unit tests: config parsing, path discovery, size calculation, rule evaluation
- Integration tests: mock filesystem with fake target/ dirs
- No real cargo-sweep calls in tests; mock Command output

## Decisions

- **Approach A (Build Steward module)** chosen over thin wrappers or mise-only
- **All four tools** integrated (cargo-sweep, cargo-wipe, cargo-clean-all, sccache)
- **Dry-run default** for all destructive operations (safety-first)
- **Daemon integration** for auto-sweep with configurable rules
