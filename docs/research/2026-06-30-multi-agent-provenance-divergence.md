---
title: Multi-Agent Provenance + Divergence Read Model
description: A consolidated-view read model that federates every agent's take on an entity, tags provenance, and surfaces divergence as signal
version: '1.0'
updated: 2026-06-30
type: research
category: architecture
phase: phase3
status: draft
audience: builders
tags: [multi-agent, provenance, divergence, daemon-truth, ipc, conflict-history]
---

# Multi-Agent Provenance + Divergence Read Model

## Seed insight

A multi-agent research dashboard's durable value was its **data model**, not its
UI. Four properties make it durable:

1. **Provenance** — every fact/decision is tagged with the agent that produced
   it. Nothing anonymous.
2. **Per-entity federation** — for a given entity (file, module, decision),
   surface *every* agent's take side-by-side, never pre-merged.
3. **Divergence-as-signal** — when two agents made opposing decisions on the
   same entity, the disagreement *is* the insight.
4. **Hard gates > reweighting** — model constraints as gates, not soft weights.

These map cleanly onto primitives Impulse already ships. This note specifies a
`consolidated-view` read model that extends them.

## Grounding: what already exists (verified against code)

| Claim | Status | Evidence |
|---|---|---|
| `impulse-ops` canonical agent registry | **Confirmed** | `impulse-rs/impulse-ops/src/agent_registry.rs:1` — "Canonical agent registry — a single, data-driven source of truth"; `AgentDescriptor` catalog + `AgentRegistry::builtin()` (`:139`), resolve/get by slug (`:303`, `:308`). |
| Tracks "who did what across sessions" via HISTORY.jsonl | **Confirmed, with caveat** | `impulse-rs/src/state/persistence.rs:31` `HistoryEntry { session_id, session_name, platform: Option<Platform>, files_touched, tools_used, ... }`. **Caveat:** provenance here is coarse — keyed by `Platform` enum + `session_id`, *not* the canonical registry slug. The read model must resolve `Platform`/command → `AgentDescriptor` via the registry. |
| Decisions in GENOME.md | **Partially confirmed** | `GENOME.md` is JSON-serialized (despite the `.md` name) and **has two typed read paths today**: `impulse-rs/src/handlers/memory.rs:24` reads `read_json::<memory::Genome>("GENOME.md")` (round-tripped by `add_decision`, `:86-88`), and `impulse-rs/src/ops_workbench.rs:683-691` `load_genome` deserializes it via `serde_json::from_str` into `GenomeFile`. The real gap is narrower: `memory::Decision { date, description, rationale, tags: Vec<String> }` (`src/memory/mod.rs:21`) carries **no dedicated agent field** — only `tags` — and the ops `GenomeDecision` projection (`ops_workbench.rs:39`) drops `tags` entirely, so decision provenance is not yet structured per-agent. Insight-level provenance *does* exist: `impulse-ops/src/lib.rs:157` `InsightRecord { agent_label, kind, content, timestamp }`. |
| Conflict-history IPC exists | **Confirmed** | `protocol.rs:185` `GetConflictHistory`, `:187` `ClearResolvedConflicts`; handler `handlers.rs:853` (returns `respond_ok(&resolver.get_resolution_history())`); client `client/mod.rs:299` `get_conflict_history`; the shared enum lives in `impulse-ops/src/lib.rs:25` `WorkbenchDaemonRequest` and is held in lockstep by the `assert_shared_request_compatible` test (`protocol.rs:726`, conflict-history assertion at `:815`). |
| Divergence view is a natural extension of conflict-history | **Confirmed, with caveat** | `agent/coordinator.rs:132` `ConflictRecord { file_path, resolution: ConflictResolution, resolved_at, panes_involved: Vec<String> }`; `ConflictResolution` = `Merge \| AcceptTheirs \| AcceptMine \| Rebase` (`:13`). **Caveat:** records are keyed by `panes_involved` (pane label strings), not agent identity. Today's "divergence" is "two *panes* edited the same file." Lifting it to "two *agents* made opposing *decisions* on an entity" requires (a) pane/session → agent-slug resolution and (b) a richer stance than a 4-variant file-merge enum. |
| daemon-truth wrestles with LIVE staleness | **Confirmed** | `docs/ROADMAP-PLAN.md:153-155` — mark telemetry stale after 10s without heartbeat, stop overlaying after 10s, purge telemetry-only entries after 60s; match telemetry to durable agents by `session_id` then agent `id` (`:149`). Durable snapshot is built first from sessions/history/genome/retrieval/artifacts; fresh telemetry *overlays* (`:111-113`). |
| Aligns with `do-not-unify` | **Confirmed** | The read model federates at read time and keeps each agent's contribution separately queryable; it never collapses to a single proxy narrative. |

**Net correction:** Impulse has provenance at the *insight* level (`agent_label`)
and coarse provenance at the *session/file* level (`Platform`), plus pane-level
conflict history. It does **not** yet have (1) registry-slug-resolved provenance
on history/decisions, or (2) an entity-keyed federated view. Those are the two
gaps this feature closes.

## Design: the `consolidated-view` read model

### Shared types (new, in `impulse-ops` — the shared-types crate)

```rust
// EntityKey identifies the thing every agent has an opinion about.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityKey { pub kind: EntityKind, pub id: String } // id e.g. "src/daemon/handlers.rs"

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind { File, Module, Decision }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionSource { History, Genome, Conflict, Insight, LiveTelemetry }

// One agent's take on one entity. Provenance is the canonical registry slug.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentContribution {
    pub agent_id: String,       // canonical AgentDescriptor slug (resolved via registry)
    pub agent_label: String,    // human label
    pub source: ContributionSource,
    pub stance: String,         // the take/decision (e.g. "AcceptMine", "use Dioxus host")
    pub detail: String,         // free text / rationale
    pub recorded_at: Option<DateTime<Utc>>,
    pub settled: bool,          // true => from a durable store; false => in-flight telemetry
    pub stale: bool,            // true => telemetry past the 10s heartbeat window
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Divergence {
    pub divergent: bool,        // HARD GATE: only true for >=2 settled, non-stale opposing stances
    pub axis: String,           // what they disagree about
    pub opposing: Vec<(String, String)>, // (agent_id, stance)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsolidatedEntityView {
    pub entity: EntityKey,
    pub contributions: Vec<AgentContribution>, // federated, NEVER pre-merged
    pub divergence: Option<Divergence>,
    pub snapshot_at: DateTime<Utc>,
    pub has_inflight: bool,     // any non-settled contribution present
}
```

All derive `Serialize + Deserialize` → each gets a serde round-trip test
(`CLAUDE.md` testing bar). No `unwrap` on production paths; errors flow as
`OpsError`/`anyhow` with `.context(...)`.

### IPC method (new)

Add one request + mirror it across both protocol enums (the codebase keeps
`DaemonRequest` and `impulse_ops::WorkbenchDaemonRequest` — defined in
`impulse-ops/src/lib.rs:25` — in lockstep via the `assert_shared_request_compatible`
test, `protocol.rs:726`, which already pins `GetConflictHistory` at `:815`; a new
`GetConsolidatedView` arm must be added there too or the test fails):

```rust
// protocol.rs — DaemonRequest
/// Federated, provenance-tagged view of every agent's take on one entity.
GetConsolidatedView { entity: impulse_ops::EntityKey },
```

Handler (mirrors the `GetConflictHistory` arm at `handlers.rs:853`), returns
`DaemonResponse::Ok { result }` via the existing `respond_ok(&view)` helper:

```rust
DaemonRequest::GetConsolidatedView { entity } => {
    let registry = AgentRegistry::registry_for_runtime().unwrap_or_default();
    let view = build_consolidated_view(&entity, &state, &conflict_resolver, &registry).await?;
    respond_ok(&view)
}
```

`request_type_name` gets a `"GetConsolidatedView"` arm (`protocol.rs:282`), and
`client/mod.rs` gets a typed `get_consolidated_view(entity)` wrapper paralleling
`get_conflict_history` (`:299`).

### How it reads from existing stores (federation, no merge)

`build_consolidated_view` is a pure-ish aggregator that *appends* one
`AgentContribution` per (agent, source) — it never coalesces:

| Source | Read path | Stance | Settled |
|---|---|---|---|
| `History` | `State::get_history_sync()` → `HistoryEntry` whose `files_touched` contains the entity id; `platform` → `registry.resolve()` → slug | "touched / `tools_used`" | `true` |
| `Conflict` | `ConflictResolver::get_resolution_history()` → `ConflictRecord` for the file; each `panes_involved` pane → session → agent slug | `resolution.as_str()` (e.g. `accept_mine`) | `true` |
| `Insight` | `InsightRecord { agent_label, kind, content }` where `kind == "decision"` and content references the entity | `content` | `true` |
| `Genome` | deserialize the **typed JSON** `memory::Genome` via `storage().read_json` (the path `handlers/memory.rs:24` already uses — *not* a markdown line parse, which would silently omit every persisted decision); iterate `genome.decisions` (`Decision { description, rationale, tags }`). Agent slug must come from `tags` → `registry.resolve()`; decisions aren't agent-tagged today, so degrade gracefully → `agent_id = "unknown"` | decision text | `true` |
| `LiveTelemetry` | `TerminalOpsReport` overlay (the daemon-truth ephemeral layer) matched by `session_id` then agent `id` | current action | `false` |

Provenance rule: **every** contribution carries a registry-resolved `agent_id`.
If resolution fails, `agent_id = "unknown"` (never silently dropped — that would
violate property 1).

### Live staleness — reuse the daemon-truth overlay rules verbatim

This is the same problem `ROADMAP-PLAN.md:153-155` already solved for the
ops snapshot. We reuse it, we do not invent a second policy:

- Durable contributions (`History`/`Conflict`/`Insight`/`Genome`) are `settled = true`.
- Telemetry contributions overlay onto the durable set, matched by `session_id`
  then agent `id`, and are `settled = false`.
- A telemetry contribution past **10s** without heartbeat → `stale = true` and is
  **no longer overlaid** into `contributions` (dropped from the federated list).
- Telemetry-only entities are **purged after 60s**.
- `snapshot_at` stamps the read so the renderer can show overlay age.

## The hardest engineering problem (stated honestly)

**Live divergence rendering without misrepresenting settled-vs-in-flight state.**

The trap: agent A has a *settled* HISTORY decision "accept mine" on
`handlers.rs`, while agent B is *in-flight* (telemetry) editing the same file
right now. Naively diffing their stances renders a red "DIVERGENCE" banner — but
B's stance is a 10-second-lived overlay that may evaporate before it ever becomes
a decision. That produces **phantom conflicts**: the UI screams disagreement over
something that was never settled. Conversely, suppressing all in-flight stances
hides the *one moment* where divergence is actionable (you can still intervene).

Resolution — apply property 4 (**hard gates > reweighting**) literally:

1. **Two tiers, never blended.** `contributions` carries both settled and
   in-flight items, each self-describing via `settled`/`stale`. The renderer
   draws settled contributions solid and in-flight ones dimmed/italic with an
   age badge. The data model forbids conflation; the UI honors it.
2. **The `divergence.divergent` flag is a HARD GATE.** It is `true` **iff** there
   are ≥2 **settled, non-stale** contributions holding opposing stances. In-flight
   telemetry can *never* set the hard gate. This is exactly "gates, not weights":
   no confidence score, no averaging, no "70% divergent."
3. **In-flight disagreement is a separate, softer signal** — a
   `has_inflight` boolean + a dim "provisional" hint — surfaced as
   *coordination opportunity*, not as *settled divergence*. It routes to the
   existing `Recommendation`/`CrossPaneSync` path, not the divergence gate.
4. **Determinism under churn.** Because the hard gate ignores the volatile layer,
   the divergence verdict only changes when a durable store changes — it does not
   flicker on every 10s telemetry tick. The read is idempotent for a fixed
   durable state regardless of overlay timing.

This keeps the federated view *live* (you see who is touching the entity now)
while never *lying* about what is settled.

## Surfacing

- **ratatui TUI**: an entity-detail pane listing contributions grouped by
  `agent_id`, settled solid / in-flight dimmed, with a divergence gate banner
  only when `divergence.divergent == true`. Mirrors the existing conflict-history
  view in `src/ui/`.
- **Dioxus desktop host**: renders the same `ConsolidatedEntityView` snapshot
  (desktop surfaces render daemon snapshots, not local shadow truth — per
  `ROADMAP-PLAN.md`).

## Pattern compliance

- Atomic writes: read-only feature — no new persisted file; telemetry stays
  ephemeral daemon memory (consistent with `ROADMAP-PLAN.md` "not a new persisted
  file").
- `Result<T>` everywhere; `build_consolidated_view` returns `anyhow::Result`,
  type errors via `OpsError`/`thiserror`; no `unwrap` outside tests.
- Shared types live in `impulse-ops`; both protocol enums stay compatible
  (guarded by `assert_shared_request_compatible`).
- Tests: serde round-trip per new type; `Err`-path test on `build_consolidated_view`;
  a hard-gate test proving in-flight stances never flip `divergent`.

## Build sequence (dependency order, not time)

1. Land shared types in `impulse-ops` (+ round-trip tests) — unblocks everything.
2. Add registry resolution helpers `Platform`/`session` → `agent_id`.
3. Implement `build_consolidated_view` aggregator over the four durable sources.
4. Wire the telemetry overlay (reuse existing 10s/60s overlay code).
5. Add `GetConsolidatedView` to both protocol enums + handler + client wrapper.
6. Surface in ratatui, then Dioxus host.

Step 5 blocks on 1; step 3 blocks on 2; steps 1–4 are the critical path. The
hard-gate test (property 4) is the acceptance gate for step 3.

## Cross-anchor synthesis

This note was written as one of three parallel anchor studies (Impulse, ROSA,
and a portable shared spec). Reading them together sharpened three things:

- **The primitive is real in three codebases, not one.** ROSA reached the same
  four properties against entirely different machinery (a closed `SurfaceKind`
  catalog, a `risk_floor` guardrail, an `OperatorTakeaway` compiler). When two
  independent analyses converge on *settled-facts-only divergence* + *monotonic
  gates*, the contract is sound rather than Impulse-specific.
- **Convergent teeth.** All three anchors independently landed on: divergence is
  **derived at read time from settled facts only, never authored or averaged**,
  and gates **eliminate rather than soften** (here: in-flight telemetry can never
  flip `divergent`; in ROSA: a vetoed option is removed *before* ranking; in the
  shared spec: no `consensus()` write path exists). This is the structural
  enforcement of the `do-not-unify` rule.
- **Sequencing.** Impulse's value is gated on a prerequisite the other anchors
  don't have: **registry-slug provenance resolution** (build sequence step 2) and
  structured GENOME decision provenance. Both are worth doing on their own merits
  regardless of this feature. Recommended order across the program: ROSA first
  (cleanest fit — payload-only addition to an existing surface), the Impulse
  provenance bridge in parallel as a standalone, then the `consolidated-view`
  drops on top with its hardest problem (live flicker) already solved above.

**Sibling notes:**
- Portable contract (binds both adapters): `~/.ai-memory/docs/multi-agent-provenance-divergence-spec-2026-06-30.md`
- ROSA operator-surface proposal: `~/code/ROSA_RenewBuild/architecture docs/ROSA-multi-agent-operator-surface-proposal.md`
- Cross-project decision logged: `~/.ai-memory/core/memory/decisions.md` (2026-06-30)
