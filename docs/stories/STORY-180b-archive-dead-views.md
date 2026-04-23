---
id: STORY-180b
title: Archive 3 dead views and the dead GuardrailsView field
spec: SPEC-EGUI-RETIRE
status: done
priority: high
created: 2026-04-23
completed: 2026-04-23
commit: 100389c
---

# Archive dead views (Plan 2 / Loop 180b)

## Outcome

Per "archive, don't delete" — moved `guardrails.rs`, `context.rs`, and
already-gone `artifacts.rs` to `_archive-2026-04-23-L180b/`. Removed
`pub mod guardrails;` from `views/mod.rs` and the dead
`guardrails: GuardrailsView` field from `app.rs`. `GuardRule` struct in
`ipc/types.rs` marked `#[allow(dead_code)]` with justification — kept as
typed scaffold for future Dioxus port.

## Acceptance

- [x] Files moved (not deleted) to `_archive-2026-04-23-L180b/`
- [x] `cargo build --workspace` clean
- [x] `cargo test --workspace`: 2,005 → 1,993 (-12 = guardrails view tests, expected)
- [x] All gates green
