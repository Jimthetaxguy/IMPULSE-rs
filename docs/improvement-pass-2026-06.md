# Hardening & Multi-Agent Improvement Pass — June 2026

A focused engineering pass across the `impulse-rs` workspace. Every change below
was committed individually and verified against the full gate
(`cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`);
the workspace finished green (1,600+ tests passing, zero clippy warnings).

## Security

- **Path-traversal sandbox escapes closed.** Three write paths built filesystem
  paths from caller-supplied components without sanitization — the dynamic-tool
  sandbox (`tooling`), orchestration handoff filenames (`orchestration`), and
  queued cleanup proposals (`stewardship`). A tool/agent could escape its
  allowed roots and write arbitrary files. Fixed at each source, plus a
  systematic audit of every `join(format!(…))` / `PathBuf::from(arg)` site
  confirming the rest are allowlist-based, enum-derived, or already validated.
- **Guardrail bypass fixed.** The built-in `block-force-push-main` rule only
  matched when the force flag preceded the branch, so `git push origin main
  --force` slipped through. Rewritten to match force + `main` in any order
  (linear-time regex, no ReDoS), preserving false-positive guards like a
  `maintenance` branch.
- **External-call timeout sweep.** Every outbound call that could hang the
  daemon or CLI is now bounded: LLM/webhook/model-fetch HTTP clients, the `sem`
  subprocess (with concurrent pipe draining to avoid deadlock), secrets-manager
  CLI fetches, and the daemon IPC client. A wedged daemon, hung API, stuck
  secrets backend, or frozen subprocess can no longer hang Impulse.

## Multi-agent monitoring & interop

- **Recognizes more coding agents.** Detection, intent classification, and PTY
  output parsing now understand Claude Code, Codex, OpenCode, Gemini, and Cursor
  — including each agent's file-operation phrasing — instead of only Claude's.
- **Drive any agent via auth-login.** Harness-mode delegation supports Claude
  Code (`--print`), OpenCode (`run`), Codex (`exec`), and Gemini (`-p`), each
  using the agent CLI's own login. (This also corrected a prior OpenCode
  mis-invocation.)
- **Richer coordination signals.** File-conflict recommendations annotate what
  each agent is doing to the shared file; routing and swarm-pattern confidence
  now reflect actual match strength instead of a fixed value; the capabilities
  manifest advertises which agents Impulse can monitor and drive.
- **Delegation tracking wired end-to-end.** `RegisterDelegation` /
  `CompleteDelegation` / `ListDelegations` are backed by a real
  `DelegationTracker` (depth limits, handoff prompts, stale-handoff detection).

## Reliability & data integrity

- Post-compaction context-refresh repaired (threshold ladder re-arms and usage
  is measured from a post-compaction baseline rather than a monotonic total).
- Append-only logs tolerate a crash-torn trailing line instead of becoming
  unreadable; the persistence dirty-flag is now lock-free and infallible.
- File-conflict detection counts distinct panes (a single agent editing a file
  repeatedly is no longer a false conflict).

## Credentials & configuration

- The environment credential provider resolves arbitrary secrets (tokens,
  connection strings), not just `*_API_KEY` values.

## Quality

- Added coverage where it was missing and risky: desktop runtime lifecycle
  guards, retrieval wire-format round-trips (catching `as_str` ↔ serde drift),
  and regression tests for every fix above.
