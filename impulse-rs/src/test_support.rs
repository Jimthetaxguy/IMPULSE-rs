//! Shared test-only synchronization helpers (impulse-rs lib crate).
//!
//! `cargo test` runs all unit tests for this crate in one process on
//! multiple threads. A test that mutates a process-global env var used by
//! more than one module must serialize on the SAME lock — a per-file
//! `static ENV_LOCK` only serializes tests within that one file, not across
//! files, which silently reintroduces the exact race the pattern exists to
//! prevent. `ION_GATE_LAUNCHER` (`impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV`)
//! is mutated by tests in `handlers::ion`, `ion_repl` (mod.rs), and
//! `ion_repl::tool_verify` — all three share this lock (TUI_SPEC.md T7).

#![cfg(test)]

/// Acquire the process-wide lock guarding mutation of `ION_GATE_LAUNCHER`
/// (and any other env var shared across these test modules). Poison-safe:
/// a prior test panicking while holding the lock must not deadlock every
/// subsequent test.
pub(crate) fn ion_gate_launcher_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
