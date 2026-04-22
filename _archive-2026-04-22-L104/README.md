# Archive — Loop 104 (2026-04-22)

Archived in Loop 104 per MiniMax cleanup instructions + cleanup audit findings:

## src-tauri/
**Why archived:** Contained only Cargo build artifacts under `target/debug/build/tauri-*/out/permissions/`. No source code. Never a workspace member. CLAUDE.md already noted "checked-in `src-tauri/` sources are effectively absent."

**First-principles rule enforced:** #3 One Language (removing dead Tauri path toward pure-Rust Dioxus future).

## impulse-shell-ui/
**Why archived:** Abandoned Dioxus 0.7.5 prototype. Never a workspace member. Marked `STATUS: PARTIAL` in its own source comment. No imports from active crates.

**First-principles rule enforced:** #3 One Language (no half-migrated modules).

## Restore
If needed: `mv _archive-2026-04-22-L104/<dir> ./` from workspace root.
