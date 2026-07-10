---
title: Routine Primitive + Daily-Brief Built-in — Phase 1 Design
description: First vertical slice of the agent-OS knowledge ecosystem — a rigid Routine primitive with the daily brief as its first built-in instance; dependencies, frameworks, tests, and goals
version: '0.1'
updated: 2026-06-30
type: research
category: architecture
phase: phase3
status: draft
audience: builders
tags: [routine, daily-brief, calendar, weather, sensors-brains, provenance, phase1, spec]
---

# Routine Primitive + Daily-Brief Built-in — Phase 1 Design

> Draft spec / pre-build. Standalone Rust crate (working name **`routines`**) built on the agent-native
> starter kit. Instantiates the aligned spine
> ([2026-06-30-program-architecture-aligned-spine.md](2026-06-30-program-architecture-aligned-spine.md)).
> The crate is not yet scaffolded — this doc is the design gate. On approval, this spec moves into
> `<crate>/docs/`.

## 1. Goal & success criteria

A working vertical that proves the whole spine end-to-end with minimal surface:

- `routines run daily-brief --tenant james` reads **real** calendar + **real** weather, runs the
  brief brain, writes a **provenance-tagged** KB entry, and prints the summary.
- `routines query --tenant james` returns today's brief from the KB.
- The brief is expressed as a **`Routine` declaration**, executed by a generic engine — not hardcoded.
- Every capability is drawn from a **closed registry**; every KB write is provenance-stamped; the KB
  is **partitioned by tenant**.
- The brain sits behind a `DailyBriefBrain` trait so the Phase-2 LLM swap is a drop-in.
- Tests green: unit (sensors, brain, engine, KB) + one real-systems integration test.

## 2. The Routine primitive (rigid schema, flexible instances)

```
Routine {
  id: slug,                 // "daily-brief"
  tenant: TenantId,         // multi-tenancy from day one
  trigger: Explicit | Scheduled | Event,   // Phase 1 uses Explicit only
  steps: [StepRef],         // ordered capability invocations, each from the closed registry
  output: KbWrite | Present,
}
StepRef { capability: slug, params: Value }   // capability MUST exist in the registry (R1)
```

Built-in routine = a `Routine` shipped in code/config. User-defined routine (Phase 2) = a `Routine`
authored at runtime, **validated against the registry** (can only reference existing capabilities) and
gated. Daily-brief built-in:

```
Routine { id: "daily-brief", tenant, trigger: Explicit,
  steps: [ {capability:"calendar.today"}, {capability:"weather.forecast"},
           {capability:"brain.daily-brief"} ],
  output: KbWrite + Present }
```

## 3. Components (interface-first units)

| Unit | Responsibility | Depends on |
|---|---|---|
| `Registry` | Closed catalog of capability slugs → impls; provenance source | — |
| `Sensor` trait | `async read(ctx) -> SensorReading` (read-only) | — |
| `CalendarSensor` (`calendar.today`) | Federate today's events across a tenant's enabled accounts | `CalendarSource` per account |
| `CalendarSource` trait / `GoogleCalendarSource` | Per-provider calendar read for one account | provider access (§4), `SecretStore` |
| `WeatherSensor` (`weather.forecast`) | Forecast for the tenant's home location | weather.gov |
| `Config` | Per-tenant config loader (accounts, home, timezone, model-role); versioned, validated | filesystem (XDG) |
| `SecretStore` trait | Resolve `SecretRef` → secret at runtime (Infisical / Keychain adapters) | Infisical / Keychain |
| `DailyBriefBrain` trait | `process(events, forecast) -> DailyBrief` | sensor output types |
| `RuleBasedBrief` impl (`brain.daily-brief`) | Deterministic rules (outdoor event + precip → flag) | — |
| `LlmProvider` trait | `complete(prompt, opts) -> Completion` — provider-agnostic LLM seam | — |
| `LlmBrief` impl (Phase 2) | `DailyBriefBrain` backed by any `LlmProvider` | `LlmProvider` (not a concrete SDK) |
| `KnowledgeBase` | Write/read markdown KB entries w/ provenance, per tenant | filesystem |
| `RoutineEngine` | Execute a `Routine`: run steps → brain → write output | all of the above |
| CLI (`routines` bin) | `run <routine> --tenant`, `query --tenant <q>` | engine |

**Capability kinds + risk tiers (T0–T4):** `weather.forecast` is a **sensor / T0** (public read,
ungated). `calendar.today` is a **sensor / T1** (*private* read — not free: scoped by tenant, fields
minimized before they reach the brain). `brain.daily-brief` is a **brain** at `execution_tier=code`
(Phase 1) → `micro-agent` (Phase 2). KB write is an **internal sink / T2** (owned local write,
idempotent, provenance-stamped) — not an external actuator. No T3/T4 actuators in Phase 1. Each
capability's registry entry carries `{kind, execution_tier, risk_tier, required_scopes,
provenance_required}`.

### 3.1 Modularity & LLM-agnosticism (non-negotiable)

**Every external boundary is a trait; the core depends on no concrete provider or source.**

- **Traits define the seams** (all in `routines-core`): `Sensor`, `CalendarSource`, `WeatherSource`,
  `DailyBriefBrain`, **`LlmProvider`**, `KnowledgeStore`. The `RoutineEngine` knows only the traits.
- **Concrete adapters are optional, feature-gated modules** — e.g. `llm-claude`, `llm-openai`,
  `llm-ollama` (local), `cal-gws`, `cal-ics`. **Building the core pulls in zero of them.** A provider
  is added by writing an adapter, never by editing the engine.
- **Selection is config-driven (policy-as-data), not compile-time.** A small `config` names which
  `CalendarSource`, which `LlmProvider`, model/role, home location, etc. Swapping Claude → local
  Ollama is a config change, not a recompile of logic. Model is referenced by **role**, not a hard
  model id (e.g. `brief-writer`), resolved from config (ion spec-a `model_role`).
- **`LlmProvider` trait is defined in Phase 1** even though no impl ships until Phase 2 — so the seam
  exists and the LLM swap is a pure drop-in. The trait is intentionally minimal:
  `complete(prompt, opts) -> Completion` (+ optional `stream`), nothing provider-specific leaking
  through.

### 3.2 Accounts & providers (multi-account, multi-provider-ready)

A tenant owns many **accounts**; each account belongs to a **provider**. These are three distinct
concepts — keep them crisp:

- **Tenant** — the person whose KB this is (James, Tiffany).
- **Account** — one credentialed external connection a tenant owns (`james-personal-gmail`,
  `james-work-gmail`). `Account { id, tenant, provider, label, credentials_ref: SecretRef, enabled }`.
- **Provider** — the service type. `ProviderKind` is a **closed enum** (`google`, `microsoft`,
  `icloud`, …) — adding one is a governance event (a new adapter); **accounts of a known provider are
  runtime config** (R1: classes closed, instances runtime).

`CalendarSource` is implemented **per provider** (`GoogleCalendarSource`). `CalendarSensor` resolves
the tenant's enabled calendar accounts, dispatches each to its provider's source, and **federates** —
every event is tagged with its `account_id` + `provider`, **never merged into an anonymized blob**
(`do-not-unify`). The `Account` model is **not calendar-specific** — future email/finance sensors
reuse it.

- **Phase 1 ships:** the Account model + **N Google accounts** (proves multi-account federation).
- **Phase 1 interface-ready, not impl'd:** non-Google providers — `ProviderKind`/`CalendarSource` are
  there; only the Google adapter ships.

### 3.3 Config & secrets — James's data out of the base code

**The base code ships zero personal data.** No accounts, tokens, file paths, home location, model
ids, or `/Users/...` hardcoded anywhere (`real-systems-only`). Personal data lives in two external
layers:

- **Config layer (non-secret).** Per-tenant config (XDG dir, e.g. `~/.config/routines/<tenant>.toml`)
  declaring accounts (provider, label, `secret_ref`, enabled), home location, timezone, and
  provider/model-role selection. **Policy-as-data:** carries a `version`, serde-validated on load,
  gitignored, never committed.
- **Secret layer.** A `SecretStore` trait resolves a `SecretRef` → secret. Adapters: **Infisical** via
  the **official `infisical` Rust SDK** (machine-identity Universal Auth; creds from
  `INFISICAL_CLIENT_ID`/`INFISICAL_CLIENT_SECRET` env, never config/disk) and **macOS Keychain** (their
  MCP-hardening pattern, via the `security` CLI). Because the SDK is async, secrets are **eagerly
  pre-resolved once at startup** (`bootstrap()` → `PreloadedSecretStore`) so the trait stays sync and
  the network round-trip happens once, not per-account. Config holds only the *reference* (e.g.
  `infisical://ROUTINES_JAMES_GOOGLE_PERSONAL`) plus a non-secret `[infisical]` block (project_id,
  environment), never plaintext. This code **never writes a secret to disk**. Project-prefixed names
  (`ROUTINES_*`). The `infisical` crate is v0.0.x — pinned behind the `secret-infisical` feature with a
  CLI shell-out fallback documented if the early API is insufficient.
- **Per-tenant isolation:** each tenant has its own config file + its own secret namespace. James and
  Tiffany never share config or secrets.

## 4. Dependencies (real systems)

- **Calendar access — RESOLVED: (b) native Rust Google Calendar API client.** A first-party in-crate
  client (reqwest + rustls) calling the Calendar v3 API with **OAuth in-crate** (read-only scope
  `calendar.events.readonly`, token refresh owned by the crate). Chosen over the `gws` shell-out for a
  fully typed seam, no external-process dependency, and per-account control. This is a larger lift
  (OAuth flow + refresh + token-expiry error surfacing) — handled inside `GoogleCalendarSource`, behind
  the `CalendarSource` trait so `.ics` / other providers swap later. Scope: **read-only**. Tokens are
  resolved via `SecretStore` (never on disk); token-expiry is a distinct error variant (§11.5/.6).
- **Secrets — Infisical + macOS Keychain** via the `SecretStore` trait. No plaintext credentials in
  config or code; refs only.
- **Weather — weather.gov** (`api.weather.gov`, free, no key, US-only): `/points/{lat,lon}` →
  forecast URL → periods. Default **home location: Long Island City, NY** ([home-geohash-at-export-boundary]),
  config-overridable.
- **Rust toolchain** (present).
- Phase 2 only: **any LLM provider** behind the `LlmProvider` trait — Claude / OpenAI / local
  (Ollama/MLX) — chosen by config, never hardcoded. No single-vendor dependency enters the core.

## 5. Frameworks / crates

- `tokio` — async runtime; `reqwest` (rustls) — HTTP for weather (+ calendar if direct API).
- `serde` + `serde_json` + `serde_yaml` — Routine declarations + KB frontmatter.
- `chrono` / `chrono-tz` — dates, times, timezone for "today".
- `clap` — CLI; `anyhow` + `thiserror` — errors (fail loud).
- KB storage: **markdown files first** (human-readable, git-friendly, provenance in YAML frontmatter).
  Defer `rusqlite` index until query needs more than "read today" (YAGNI).
- Test: `cargo test`; `httpmock`/`wiremock` for sensor unit tests (HTTP mocked at the boundary, in
  test files only — permitted by `real-systems-only`).
- **Structure (modular):** start as a single crate with feature-gated adapter modules (`llm-*`,
  `cal-*`) so the core compiles with none; graduate to a Cargo **workspace**
  (`routines-core` / `-sensors` / `-brains` / `-llm` / `-cli`) if the pieces want independent reuse
  by ROSA/IMPULSE. Core crate has **no** provider/source crate in its default dependency set.

## 6. Data flow

```
CLI run daily-brief --tenant james
  → Config.load(james)  → accounts, home, timezone, model-role
  → Engine loads "daily-brief" Routine
  → CalendarSensor.read  (fan out over james's enabled accounts, each via its provider source;
        SecretStore resolves each account's token; FEDERATE — events tagged account_id+provider;
        one account failing = fail-soft, recorded as a gap)   ‖   WeatherSensor.read
  → DailyBriefBrain.process(events, forecast) -> DailyBrief
  → KnowledgeBase.write(entry{ tenant, date, provenance:[ {calendar.today, account, provider},
        weather.forecast, brain.daily-brief ], gaps:[…], recorded_at, body })   // idempotent per (tenant,date)
  → print summary
CLI query --tenant james "..."  → KnowledgeBase.read(tenant) -> today's brief
```

## 7. Provenance & multi-tenancy

- Every KB entry's frontmatter carries `tenant_id`, the contributing capability **slugs** (with
  `account_id` + `provider` for account-sourced contributions), a `source_version`, and a timestamp.
  For the Phase-2 LLM brain, also `model_role` + `model_id` + `prompt_version` so a brief is
  reproducible/auditable. **Federation, never merge** (`do-not-unify`); no collapsed/consensus write-back.
- KB is partitioned by tenant (`kb/<tenant_id>/...`). A tenant can only read its own partition
  (enforced + tested). Every capability is parameterized by `tenant_id` even with one tenant today.

## 8. Tests (by execution_tier)

- **Sensors (code):** unit tests with mocked HTTP (`weather.gov` fixture; calendar fixture);
  one **integration** test behind `--features integration` hitting real `weather.gov`.
- **`RuleBasedBrief` (code brain):** deterministic unit tests — event+forecast fixtures → expected
  flags + summary.
- **`RoutineEngine`:** unit test — a `Routine` executes its steps in order and writes a
  provenance-stamped entry; an unknown capability slug is rejected (R1). The trait seams let the
  engine be tested against **fakes** (`tests/fakes/` — permitted by `real-systems-only`), incl. a
  `FakeLlmProvider`, so the LLM path is testable without any real provider.
- **`CalendarSensor` federation:** multi-account merge tags each event with `account_id`+`provider`;
  one account failing → fail-soft (others present, failure in `gaps`). `FakeCalendarSource` per account.
- **`Config` + `SecretStore`:** config validates on load (version, unknown provider rejected); a
  `FakeSecretStore` resolves refs in tests — no real Infisical/Keychain call in unit tests.
- **`KnowledgeBase`:** round-trip (write→read, provenance intact); tenant-isolation (A cannot read B);
  idempotency (two runs same day → one updated entry, not two).
- **Integration ("real systems"):** `cargo test --features integration` runs the full daily-brief
  routine against real weather (+ real calendar if available) for one tenant.
- Phase 2: **evals** for `LlmBrief` (day fixtures → expected-shape/quality brief) — not unit tests.

## 9. Non-goals (YAGNI for Phase 1)

Routine-builder agent (Phase 2) · LLM brain **impl** (Phase 2 — but the `LlmProvider` +
`DailyBriefBrain` traits ship in Phase 1) · concrete provider adapters (`llm-claude` etc., Phase 2) ·
scheduled trigger (explicit CLI only) · per-event geocoding (one home location) · SQLite index /
semantic query · actuators / sending anything outward · MCP edge exposure (CLI first).

## 10. Build dependency order (critical path, no time estimates)

1. **Routine schema + Registry + capability kinds** (rigid core) — blocks everything.
2. **Config + `SecretStore`** (load tenant config, resolve secret refs) — blocks accounts/sensors.
3. **Account model + `KnowledgeBase`** (tenancy-aware write/read, provenance) — blocks output.
4. **Sensors** — `WeatherSensor`; `CalendarSensor` federating over accounts via
   `CalendarSource`/`GoogleCalendarSource` (fail-soft). *Gated on the §4 calendar-access decision.*
5. **`RuleBasedBrief`** — depends on sensor output types (interfaces).
6. **`RoutineEngine`** — depends on 1–5.
7. **CLI** — depends on 6.
8. **Integration test** — depends on 7.

**Critical path:** `1 → 2 → 3 → 4 → 5 → 6 → 7 → 8` (with `WeatherSensor ‖ CalendarSensor` inside 4,
and `RuleBasedBrief` parallelizable with 4). Decision gate: calendar access (§4) before step 4.

## 11. Cross-cutting nuances (newly surfaced)

Things a thoughtful build needs that weren't yet explicit:

1. **Timezone correctness.** "Today" = today in the **tenant's configured timezone** (config), not
   server UTC. Event times normalized to it; weather periods are local to the home location.
   `chrono-tz`. Getting this wrong silently shows the wrong day's brief.
2. **Fail-soft sensors / partial federation.** With N accounts, one failing must **not** kill the
   brief. Include what succeeded; record failures as `gaps` in provenance (which account/source
   failed and why). Reads degrade; they never hard-fail the routine.
3. **Idempotency per day.** KB entry keyed by `(tenant, date)`. Re-running **updates** that day's
   entry (fresh provenance timestamp), never duplicates. (Versioning prior runs: deferred.)
4. **Privacy / data-minimization + provider trust.** Calendar data is sensitive; an *external* LLM
   provider would receive it. So: (a) a field-minimization seam before the brain (config: which
   fields the brain sees); (b) a **provider trust tier** in config — sensitive tenants/data may be
   pinned to a **local** provider (Ollama/MLX), external providers allowlisted only where permitted.
   Moot for the Phase-1 rule brain (no network), but the *seam + config knob* ship now so the Phase-2
   LLM brain inherits them. Ties to ROSA privacy tiers + the wire-gate.
5. **OAuth least-privilege + refresh.** Read-only calendar scope (`calendar.events.readonly`). The
   native client **owns refresh in-crate** (resolved §4): persist refresh token via `SecretStore`,
   refresh access tokens on expiry, and surface token-expiry as a *distinct* error
   ("re-auth account X"), never a silent failure.
6. **Error taxonomy (fail loud, actionable).** `thiserror` enum distinguishing config error
   (missing/invalid account) · auth error (token expired → "re-auth account X") · source error (API
   down → skip + gap) · data error (malformed). Never a silent swallow.
7. **Config schema versioning.** Config carries a `version`; validated on load; unknown provider
   rejected (R1); clear migration/error messages.
8. **Bounded concurrency / rate limits.** Account fan-out respects provider rate limits (bounded
   concurrent reads), not an unbounded blast.
9. **Untrusted content (prompt injection).** Calendar titles/descriptions are **untrusted data, never
   instructions** (OWASP LLM01). A meeting title saying "ignore previous instructions…" must be inert.
   The brain receives sensor content inside a clearly-fenced *data* channel, separate from its system
   prompt; the Phase-2 LLM brain never treats fetched text as authority. Moot for the rule brain, but
   the fencing seam ships now so the LLM brain inherits it.
10. **Observability from day one.** Thread a `run_id` + `correlation_id` through the whole routine;
    log per-step `{capability_id, capability_version, status, latency, gaps}` (no secrets, refs only).
    Cheap now, indispensable the first time a brief looks wrong. OpenTelemetry-GenAI-shaped.
11. **Ledger-derived KB (considered).** A per-`(tenant,date)` markdown entry ships in Phase 1, but the
    durable target is an **append-only event ledger as source-of-truth** with the brief as a *derived*
    view (replay/audit/eval-repeatability) — exactly IMPULSE's `HISTORY.jsonl` pattern. Phase 1 keeps
    the simple entry; the write path is shaped so a ledger can back it later.

**Phase-1 scope of these:** 1, 2, 3, 5, 6, 7, 8, 9 (fencing seam), 10 are in. #4 ships only the
*seam + config knob*; #11 ships the simple entry (ledger later). Non-Google providers,
config-migration tooling, privacy-tier *enforcement*, and the full event ledger are Phase 2+.

### 11a. Conscious Phase-1 deferrals (from the final whole-branch review, 2026-06-30)

The final review confirmed the shipped path is correct but flagged three items as gaps between the
spec's aspiration and the Phase-1 implementation. These are **deliberate deferrals**, recorded here so
Phase 2 inherits them explicitly rather than silently:

1. **Engine is a hardcoded pipeline, not step-driven.** The `RoutineEngine` R1-checks every
   `routine.steps[]` against the registry, but then always executes the fixed sequence
   weather → calendar → brain and stamps exactly those three capabilities — it does **not** dispatch
   or order execution *from* `steps`. For the single `daily-brief` built-in (whose steps match the
   pipeline) this is functionally correct and provenance is accurate. Making the engine genuinely
   step-driven (a `capability slug → executor` dispatch map so arbitrary/user-authored routines run)
   is **Phase 2** — it is the natural home for the user-defined-routine builder (already a §9 non-goal).
   Until then, "executed by a generic engine" (§2) is true for R1 membership but not for execution order.
2. **Observability seam (§11.10) not yet threaded.** `run_id`/`correlation_id` structured logging is
   deferred; Phase-1 provenance (`recorded_at` + `gaps`) covers the immediate need. First Phase-2 add.
3. **Field-minimization + prompt-fencing seams (§11.4/§11.9) are conceptual, not code.** Moot for the
   deterministic rule brain (no network, no LLM). They must ship **before** the Phase-2 LLM brain, which
   is the first consumer that sends calendar data to an external provider.

The four *fix-before-merge* findings (core must compile adapter-free; provenance must not double-count a
failed account; guard non-Google accounts; sanitize `tenant`/`date` FS ids) were **not** deferred — they
were fixed in the final consolidated fix pass before merge.

## 12. Decisions — RESOLVED (2026-06-30)

All open items resolved via the Phase-1 decision form. These are now binding for the plan:

1. **Calendar access** — ✅ **native Rust Google Calendar API client** (OAuth in-crate, read-only),
   behind the `CalendarSource` trait. (§4 updated.)
2. **Crate name** — ✅ **`routines`**.
3. **Home location** — ✅ Long Island City, NY ([home-geohash-at-export-boundary]), config-overridable.
4. **Structure** — ✅ **single crate + feature-gated adapter modules** (`cal-google`, `weather-gov`,
   `secret-infisical`, `secret-keychain`, `llm-*`); core compiles with none.
5. **Secret backend** — ✅ **both adapters now**: `SecretStore` trait with **Infisical** *and* **macOS
   Keychain** impls shipping in Phase 1, selected by config `secret_ref` scheme.
6. **Config location** — ✅ **`~/.config/routines/<tenant>.toml`** (XDG), per-tenant, gitignored.
7. **Execution mode** (post-plan) — ✅ **subagent-driven** (fresh subagent per task, two-stage review).
8. **Starter-kit enrichment** (parallel) — ✅ add **`SECURITY.md` + `EVALS.md`** to
   `~/code/_templates/agent-native-project-starter/`.
