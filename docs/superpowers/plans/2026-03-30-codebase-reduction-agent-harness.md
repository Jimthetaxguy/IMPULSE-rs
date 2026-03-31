# Codebase Reduction & Agent Harness Improvement — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove 3,000–5,000 lines of dead/duplicate code and wire the agent harness's disconnected subsystems (context lifecycle, intent classification, coordinator methods) into a fully connected intelligence loop.

**Architecture:** Four-phase approach — (1) dead code surgery removes verified unused files and methods, (2) module extraction splits 6 monolithic files (>1,000 lines each) into focused modules, (3) agent harness wiring connects the context lifecycle → agent prompts → coordinator → IPC pipeline, (4) verification adds tests and updates docs. Each phase commits independently.

**Tech Stack:** Rust, Cargo workspace (impulse-rs + impulse-ops + impulse-term + impulse-gui), tokio async runtime, serde JSON, Unix socket IPC, chrono timestamps.

---

## Parallelization Map

```
PHASE 1: Dead Code Surgery (Loops 1–7)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
                    ┌─ Task 1 (llm_backends dead files)
                    │
                    ├─ Task 2 (agent dead methods) ──┐
  Can run           │                                 │
  in parallel ──────├─ Task 3 (#[allow(dead_code)])   ├── Task 5 (handlers) ── Task 7 (commit)
                    │                                 │
                    ├─ Task 4 (intent stubs)     ─────┘
                    │
                    └─ Task 6 (notification/ops audit)

  Tasks 1, 3, 4, 6 are FULLY INDEPENDENT — no file overlap.
  Task 2 touches agent/ which Task 5 reads (handlers import agent) — run 2 before 5.
  Task 5 depends on Tasks 1-4 being complete (stable imports).
  Task 7 (commit) depends on all of 1-6.

PHASE 2: Module Extraction (Loops 9–15)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Task 8 (planning)
       │
       ├─ Task 9  (render_panels split) ─────────────────┐
       │                                                  │
       ├─ Task 10 (daemon/mod split) ─────┐               │
       │                                   │               ├── Task 13 (serialization) ── Task 15 (commit)
       ├─ Task 11 (config restructure) ───┤               │
       │                                   │               │
       └─ Task 14 (integration_tests) ────┘               │
                                                          │
          Task 12 (main.rs split) ── depends on 10 ──────┘

  Tasks 9, 11, 14 are FULLY INDEPENDENT — no file overlap.
  Task 10 (daemon split) is independent of 9, 11.
  Task 12 depends on 10 (daemon handler extraction changes main.rs dispatch).
  Task 13 depends on 9-12 (needs stable module boundaries for helpers).

PHASE 3: Agent Harness Wiring (Loops 17–24)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Task 16 (planning)
       │
       Task 17 (context→prompts) ── MUST BE FIRST
       │
       Task 18 (intent classification) ── depends on 17
       │
       Task 19 (coordinator wiring) ── depends on 18
       │
       ├─ Task 20 (conflict history IPC) ─────┐
       │                                       ├── Task 24 (commit)
       ├─ Task 21 (structured harness) ────────┤
       │                                       │
       ├─ Task 22 (session awareness) ─────────┤
       │                                       │
       └─ Task 23 (specialized IPC) ──────────┘

  Tasks 20, 21, 22, 23 are INDEPENDENT after 19 lands — can parallelize.

PHASE 4: Verification (Loops 26–30)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Task 25 (planning)
       │
       ├─ Task 26 (agent harness tests) ──┐
       │                                   ├── Task 28 (full verify) ── Task 29 (docs) ── Task 30 (final)
       └─ Task 27 (tooling tests) ────────┘

  Tasks 26, 27 are INDEPENDENT — can parallelize.
```

---

## File Structure

### Files to DELETE (Phase 1)
- `src/llm_backends/cli.rs` (671 lines) — unused CliAgent, never instantiated
- `src/llm_backends/factory.rs` (268 lines) — unused UnifiedAgent trait, never called
- `src/llm_backends/types.rs` (362 lines) — unused AgentBackend/CliProtocol/AgentConfig

### Files to CREATE (Phase 2)
- `src/ui/render_menu.rs` — menu bar + header rendering (extracted from render_panels.rs)
- `src/ui/render_tabs.rs` — tab bar + tab content rendering
- `src/ui/render_dashboard.rs` — dashboard panel rendering
- `src/ui/render_content.rs` — content panel rendering (sessions, genome, search, memory)
- `src/ui/render_status.rs` — status bar + footer rendering
- `src/daemon/protocol.rs` — DaemonRequest/DaemonResponse enums + serialization
- `src/daemon/handlers.rs` — handler dispatch functions
- `src/state/config/mod.rs` — top-level Config with nested sub-structs
- `src/state/config/retrieval.rs` — RetrievalConfig (20 fields)
- `src/state/config/context.rs` — ContextConfig (8 fields)
- `src/state/config/stewardship.rs` — StewardshipConfig (7 fields)
- `src/state/config/tools.rs` — ToolExecutionConfig (6 fields)
- `src/state/config/build_hygiene.rs` — BuildHygieneConfig (7 fields)
- `src/state/config/agent.rs` — AgentConfig (6 fields)

### Files to MODIFY (Phase 3)
- `src/agent/mod.rs` — add `query_with_context()`, session history
- `src/agent/coordinator.rs` — activate cross-pane errors + summaries in `run_local_coordination()`
- `src/agent/prompts.rs` — add `build_context_prompt()` for insight-driven prompts
- `src/context_lifecycle/extractor.rs` — populate `insight.intent` via RuleBasedClassifier
- `src/daemon/mod.rs` (or `daemon/handlers.rs`) — add new IPC endpoints

---

## PHASE 1: Dead Code Surgery

### Task 1: Remove Unused llm_backends Files (~1,300 lines)

**Files:**
- Delete: `impulse-rs/src/llm_backends/cli.rs` (671 lines)
- Delete: `impulse-rs/src/llm_backends/factory.rs` (268 lines)
- Delete: `impulse-rs/src/llm_backends/types.rs` (362 lines)
- Modify: `impulse-rs/src/llm_backends/mod.rs:7` (remove `#![allow(dead_code)]` + dead module declarations)

- [ ] **Step 1: Verify zero callers for cli.rs**

```bash
cd impulse-rs && grep -rn "cli::" src/ --include="*.rs" | grep -v "llm_backends/cli.rs" | grep -v "llm_backends/factory.rs" | grep -v "#\[cfg(test)\]"
```

Expected: ZERO results (only self-references and factory.rs which is also being deleted).

- [ ] **Step 2: Verify zero callers for factory.rs**

```bash
cd impulse-rs && grep -rn "factory::" src/ --include="*.rs" | grep -v "llm_backends/factory.rs"
```

Expected: ZERO results.

- [ ] **Step 3: Verify zero callers for types.rs unique types**

```bash
cd impulse-rs && grep -rn "AgentBackend\|CliProtocol\|CliSession\|AgentConfig" src/ --include="*.rs" | grep -v "llm_backends/types.rs" | grep -v "llm_backends/cli.rs" | grep -v "llm_backends/factory.rs"
```

Expected: ZERO results (these types are only used within the dead files).

- [ ] **Step 4: Delete the three dead files**

```bash
cd impulse-rs && rm src/llm_backends/cli.rs src/llm_backends/factory.rs src/llm_backends/types.rs
```

- [ ] **Step 5: Clean up mod.rs — remove dead module declarations and file-level allow**

Remove `#![allow(dead_code)]` from line 7 of `src/llm_backends/mod.rs`. The remaining types (Message, Role, ChatRequest, ChatResponse, Usage, LlmProvider, Agent) are all used by `src/agent/mod.rs` and `src/daemon/mod.rs`, so they'll compile clean.

Edit `impulse-rs/src/llm_backends/mod.rs`:
```rust
// REMOVE this line:
#![allow(dead_code)]

// The file should now start with:
//! LLM provider abstraction (Anthropic, OpenAI, Minimax).
//!
//! Defines the [`LlmProvider`] trait and chat interface types ([`Message`],
//! [`ChatRequest`], [`ChatResponse`]). Provider implementations live in
//! [`anthropic`].

pub use crate::error::AgentResult;
// ... rest stays the same
```

- [ ] **Step 6: Build and test**

```bash
cd impulse-rs && cargo build 2>&1 | tail -5
```

Expected: `Finished` with zero errors. If any import errors appear, they point to files we missed — grep and fix.

```bash
cd impulse-rs && cargo test 2>&1 | tail -5
```

Expected: All tests pass (the dead files had no tests that other files depended on).

```bash
cd impulse-rs && cargo clippy -- -D warnings 2>&1 | tail -5
```

Expected: Clean. Removing `#![allow(dead_code)]` may surface new warnings for serde-only fields in anthropic.rs — if so, add `#[allow(dead_code)]` on individual serde deserialize-only fields (not file-level).

---

### Task 2: Remove Unused Agent Methods (~200–400 lines)

**Files:**
- Modify: `impulse-rs/src/agent/mod.rs:304-365` (remove/gate `review_code`, `analyze_error`, `coordinate_llm`, `summarize_pane`)
- Modify: `impulse-rs/src/agent/coordinator.rs:237-243` (assess `get_resolution_history`, `clear_resolved`)

**IMPORTANT DECISION:** In Phase 3 (Task 23), we wire these specialized methods to daemon IPC endpoints. So we do NOT delete them — we mark them as `#[cfg(test)]` temporarily and restore them in Task 23. This avoids reimplementing them later.

- [ ] **Step 1: Verify review_code has zero daemon/CLI callers**

```bash
cd impulse-rs && grep -rn "review_code\|analyze_error\|coordinate_llm\|summarize_pane" src/ --include="*.rs" | grep -v "agent/mod.rs" | grep -v "#\[cfg(test)\]" | grep -v "//\|///\|/\*"
```

Expected: ZERO production callers. Only `agent/mod.rs` definitions and possibly test code.

- [ ] **Step 2: Verify ConflictResolver history methods have zero daemon callers**

```bash
cd impulse-rs && grep -rn "get_resolution_history\|clear_resolved" src/ --include="*.rs" | grep -v "coordinator.rs" | grep -v "#\[cfg(test)\]"
```

Expected: ZERO production callers.

- [ ] **Step 3: Gate specialized agent methods behind cfg(test) for now**

Edit `impulse-rs/src/agent/mod.rs` — wrap the 4 specialized methods in a `#[cfg(test)]` block:

```rust
    // Phase 3 (Task 23) will re-enable these by wiring them to daemon IPC.
    // Gated behind cfg(test) until then to keep the dead code out of production.
    #[cfg(test)]
    /// Request a code review via the LLM (API mode).
    pub async fn review_code(
        &mut self,
        pane_name: &str,
        insights: &[String],
    ) -> AgentResult<String> {
        // ... existing implementation unchanged
    }

    #[cfg(test)]
    /// Request error analysis via the LLM (API mode).
    pub async fn analyze_error(
        &mut self,
        pane_name: &str,
        error_text: &str,
    ) -> AgentResult<String> {
        // ... existing implementation unchanged
    }

    #[cfg(test)]
    /// Request cross-pane coordination analysis via the LLM (API mode).
    pub async fn coordinate_llm(
        &mut self,
        pane_summaries: &[(String, Vec<String>)],
    ) -> AgentResult<String> {
        // ... existing implementation unchanged
    }

    #[cfg(test)]
    /// Request a task summary via the LLM (API mode).
    pub async fn summarize_pane(
        &mut self,
        pane_name: &str,
        raw_output: &str,
    ) -> AgentResult<String> {
        // ... existing implementation unchanged
    }
```

- [ ] **Step 4: Build and test**

```bash
cd impulse-rs && cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -5
```

Expected: Build succeeds (no production code calls these methods). Tests that call them still compile because `#[cfg(test)]` is active during `cargo test`.

---

### Task 3: Resolve #[allow(dead_code)] Markers (17 instances)

**Files:**
- Modify: `impulse-rs/src/ops_workbench.rs:29,32`
- Modify: `impulse-rs/src/tools/python.rs:48`
- Modify: `impulse-rs/src/semantic_diff/runner.rs:170,192`
- Modify: `impulse-rs/src/llm_backends/anthropic.rs:85,94,231,344`
- Modify: `impulse-rs/src/docs/fetch.rs:29`
- Modify: `impulse-rs/src/storage/mod.rs:113,150,156,161`
- Modify: `impulse-rs/src/monty/python.rs:18`
- Modify: `impulse-rs/src/daemon/mod.rs:263,361`

- [ ] **Step 1: Categorize each marker**

For each `#[allow(dead_code)]` instance, determine:
- **Serde deserialization field** → Replace with doc comment: `/// Deserialized from API response; used via serde only`
- **Truly dead code** → Remove the code entirely
- **Test-only code** → Move behind `#[cfg(test)]`

```bash
cd impulse-rs && grep -B2 -A5 "#\[allow(dead_code)\]" src/llm_backends/anthropic.rs
```

The anthropic.rs ones (lines 85, 94, 231, 344) are serde response fields — keep them but remove the `#[allow(dead_code)]` and add `#[serde(skip_serializing)]` if they're deserialize-only. Or use `#[allow(dead_code)]` with a justification comment.

- [ ] **Step 2: Fix each file**

For each file, read the specific lines, determine category, and either:
1. Remove dead code
2. Replace `#[allow(dead_code)]` with a doc comment explaining why it exists (serde)
3. Move behind `#[cfg(test)]`

- [ ] **Step 3: Build and test**

```bash
cd impulse-rs && cargo build 2>&1 | tail -5 && cargo clippy -- -D warnings 2>&1 | tail -5
```

Expected: Zero warnings, zero `#[allow(dead_code)]` remaining (or each has inline justification).

---

### Task 4: Clean Intent Integration Stubs

**Files:**
- Modify: `impulse-rs/src/context_lifecycle/types.rs:10` (import of IntentCategory)
- Inspect: `impulse-rs/src/context_lifecycle/extractor.rs` (all insight creation sites)

**DECISION:** We KEEP the `intent: Option<IntentCategory>` field and the intent module because Task 18 (Phase 3) wires it. But we document this explicitly.

- [ ] **Step 1: Verify intent is always None in production**

```bash
cd impulse-rs && grep -rn "intent:" src/ --include="*.rs" | grep -v "intent: None" | grep -v "///\|//\|/\*\|#\[" | grep -v "intent.rs\|types.rs" | grep -v "intent_category\|intent\.as_str\|intent\.from_keywords"
```

Expected: Only `intent: None` assignments in production code. Test code may set it.

- [ ] **Step 2: Add doc comment to ExtractedInsight.intent field**

Edit `impulse-rs/src/context_lifecycle/types.rs` at the `intent` field in `ExtractedInsight`:

```rust
pub struct ExtractedInsight {
    pub pane_id: usize,
    pub agent_kind: AgentKind,
    pub timestamp: DateTime<Utc>,
    pub insight_type: InsightType,
    pub content: String,
    /// Currently always `None` — wired in Phase 3 (Task 18) to populate via
    /// `IntentCategory::from_keywords()` during extraction.
    pub intent: Option<IntentCategory>,
}
```

- [ ] **Step 3: Verify the unused portions of intent.rs**

```bash
cd impulse-rs && grep -rn "AgentIntent\|RuleBasedClassifier\|Complexity\|IntentContext\|ActivityType" src/ --include="*.rs" | grep -v "intent.rs" | grep -v "///\|//\|/\*"
```

Check which types from `intent.rs` are used outside that file. If `AgentIntent`, `RuleBasedClassifier`, `Complexity`, `IntentContext`, `ActivityType` have zero external callers, gate them behind `#[cfg(test)]` or add doc comments marking them as Phase 3 targets.

- [ ] **Step 4: Build and test**

```bash
cd impulse-rs && cargo build && cargo test 2>&1 | tail -5
```

---

### Task 5: Consolidate Handler Shared Patterns

**Files:**
- Modify: `impulse-rs/src/handlers/mod.rs` (~809 lines)
- Create: `impulse-rs/src/handlers/common.rs` (shared helpers extracted)

- [ ] **Step 1: Identify shared helpers in handlers/mod.rs**

```bash
cd impulse-rs && grep -n "^pub fn\|^pub(crate) fn\|^fn " src/handlers/mod.rs | head -20
```

Identify functions that are utility helpers (not CLI command handlers) — things like `is_truthy_env()`, `get_session_id()`, format helpers, etc.

- [ ] **Step 2: Create handlers/common.rs with extracted helpers**

Move all shared utility functions from `handlers/mod.rs` to `handlers/common.rs`. Update `handlers/mod.rs` to:
```rust
pub mod common;
pub use common::*;
```

- [ ] **Step 3: Update imports across handler files**

Grep for any handler file that calls the moved functions and update `use` paths.

- [ ] **Step 4: Build and test**

```bash
cd impulse-rs && cargo build && cargo test 2>&1 | tail -5
```

---

### Task 6: Audit notification + ops_workbench Dead Paths

**Files:**
- Inspect: `impulse-rs/src/notification/mod.rs` (909 lines)
- Inspect: `impulse-rs/src/ops_workbench.rs` (1,028 lines)

- [ ] **Step 1: Map all pub functions in notification/mod.rs**

```bash
cd impulse-rs && grep -n "^pub fn\|^pub async fn\|pub(crate) fn" src/notification/mod.rs
```

For each function, check callers:
```bash
cd impulse-rs && grep -rn "FUNCTION_NAME" src/ --include="*.rs" | grep -v "notification/mod.rs"
```

- [ ] **Step 2: Map all pub functions in ops_workbench.rs**

Same process. Check for overlap with daemon telemetry handling.

- [ ] **Step 3: Remove or gate functions with zero callers**

For truly dead functions: remove. For test-only: `#[cfg(test)]`. For functions that overlap with daemon code: add `// TODO: deduplicate with daemon/` comment and leave for now.

- [ ] **Step 4: Build and test**

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings 2>&1 | tail -5
```

---

### Task 7: Phase 1 Commit

- [ ] **Step 1: Full verification**

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

Expected: All green.

- [ ] **Step 2: Measure LOC delta**

```bash
cd impulse-rs && find . -name "*.rs" | xargs wc -l | tail -1
```

Record the new total. Compare against baseline 132,442.

- [ ] **Step 3: Stage and commit**

```bash
cd impulse-rs && git add -A
git commit -m "$(cat <<'EOF'
refactor: Phase 1 dead code surgery — remove ~X,XXX unused lines

- Remove unused llm_backends/{cli,factory,types}.rs (1,300 lines)
- Gate unused agent methods behind #[cfg(test)] pending Phase 3 wiring
- Resolve all #[allow(dead_code)] markers
- Document intent stub as Phase 3 target
- Extract handler shared patterns to handlers/common.rs
- Audit notification + ops_workbench for dead paths

LOC: 132,442 → XXX,XXX (−X,XXX)

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## PHASE 2: Module Extraction

### Task 8: Planning Checkpoint

- [ ] **Step 1: Gather metrics**

```bash
cd impulse-rs && find . -name "*.rs" | xargs wc -l | tail -1
cd impulse-rs && cargo test 2>&1 | grep "test result"
```

- [ ] **Step 2: Verify extraction targets still valid**

After Phase 1 removals, confirm the monolithic files are still the same sizes:
```bash
cd impulse-rs && wc -l src/ui/render_panels.rs src/daemon/mod.rs src/state/config.rs src/main.rs src/integration_tests.rs
```

- [ ] **Step 3: Adjust Phase 2 plans if needed**

If any file shrank significantly from Phase 1, skip its extraction or adjust the split plan.

---

### Task 9: Split render_panels.rs (2,139 lines → 5 modules)

**Files:**
- Delete (eventually): `impulse-rs/src/ui/render_panels.rs`
- Create: `impulse-rs/src/ui/render_menu.rs`
- Create: `impulse-rs/src/ui/render_tabs.rs`
- Create: `impulse-rs/src/ui/render_dashboard.rs`
- Create: `impulse-rs/src/ui/render_content.rs`
- Create: `impulse-rs/src/ui/render_status.rs`
- Modify: `impulse-rs/src/ui/mod.rs`

- [ ] **Step 1: Map function boundaries in render_panels.rs**

```bash
cd impulse-rs && grep -n "^pub(crate) fn render_\|^pub fn render_\|^fn render_" src/ui/render_panels.rs
```

Group functions by logical area.

- [ ] **Step 2: Create render_menu.rs — extract menu/header functions**

Move `render_menu_bar()` and `render_header()` to `src/ui/render_menu.rs`. Add the same imports from render_panels.rs that these functions need.

- [ ] **Step 3: Create render_tabs.rs — extract tab-related rendering**

Move tab bar and tab content rendering functions.

- [ ] **Step 4: Create render_dashboard.rs — extract dashboard panels**

Move overview/dashboard rendering.

- [ ] **Step 5: Create render_content.rs — extract content panels**

Move session, genome, search, memory panel rendering.

- [ ] **Step 6: Create render_status.rs — extract status bar/footer**

Move status bar and footer rendering.

- [ ] **Step 7: Update ui/mod.rs to re-export from new modules**

```rust
mod render_menu;
mod render_tabs;
mod render_dashboard;
mod render_content;
mod render_status;

pub(crate) use render_menu::*;
pub(crate) use render_tabs::*;
pub(crate) use render_dashboard::*;
pub(crate) use render_content::*;
pub(crate) use render_status::*;
```

- [ ] **Step 8: Delete render_panels.rs**

Only after all functions are moved and `cargo build` succeeds.

- [ ] **Step 9: Build and test**

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings
```

---

### Task 10: Split daemon/mod.rs (2,110 lines → 3-4 modules)

**Files:**
- Modify: `impulse-rs/src/daemon/mod.rs` (keep lifecycle + dispatch ~600 lines)
- Create: `impulse-rs/src/daemon/protocol.rs` (DaemonRequest, DaemonResponse enums)
- Create: `impulse-rs/src/daemon/handlers.rs` (individual handler functions)

- [ ] **Step 1: Identify protocol types to extract**

Lines 45-200 of daemon/mod.rs contain `DaemonRequest`, `DaemonResponse`, and related types. These are self-contained.

- [ ] **Step 2: Create daemon/protocol.rs**

Move `DaemonRequest`, `DaemonResponse`, `DaemonConfig`, and helper enums. Update `daemon/mod.rs`:
```rust
pub mod protocol;
pub use protocol::{DaemonRequest, DaemonResponse, DaemonConfig, PROTOCOL_VERSION};
```

- [ ] **Step 3: Create daemon/handlers.rs**

Extract individual handler functions: `handle_agent_request()`, `handle_supervisor_request()`, `handle_guard_request()`, etc. Keep the main dispatch `match` in `daemon/mod.rs` but have it call into `handlers::*`.

- [ ] **Step 4: Update all imports**

Other files that import from `crate::daemon` (like main.rs, client.rs) may need path updates.

```bash
cd impulse-rs && grep -rn "use crate::daemon::" src/ --include="*.rs" | head -20
```

- [ ] **Step 5: Build and test**

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings
```

---

### Task 11: Restructure state/config.rs with Nested Structs

**Files:**
- Replace: `impulse-rs/src/state/config.rs` (1,509 lines → ~400 + sub-modules)
- Create: `impulse-rs/src/state/config/` directory with sub-modules

- [ ] **Step 1: Create RetrievalConfig sub-struct**

Group the 20 `retrieval_*` fields into `RetrievalConfig`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrievalConfig {
    pub mode: String,
    pub backend: String,
    pub default_limit: usize,
    pub similarity_threshold: f32,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub python_cmd: String,
    pub vector_enabled: bool,
    pub semantic_strategy: String,
    pub query_timeout_secs: u64,
    pub index_timeout_secs: u64,
    pub batch_size: usize,
    pub candidate_pool: usize,
    pub deduplicate_enabled: bool,
    pub fuzzy_matching_enabled: bool,
    pub experimental_pageindex_enabled: bool,
    pub pageindex_mode: String,
}
```

- [ ] **Step 2: Use #[serde(flatten)] for backwards compatibility**

```rust
pub struct Config {
    pub log_level: String,
    // ... top-level fields stay ...

    #[serde(flatten)]
    pub retrieval: RetrievalConfig,

    #[serde(flatten)]
    pub context_injection: ContextInjectionConfig,
    // ... etc
}
```

**CRITICAL:** `#[serde(flatten)]` preserves the flat JSON structure. Existing config.json files with `retrieval_mode`, `retrieval_backend` etc. must still deserialize correctly. However, the flattened field names must match exactly. Test this with a round-trip.

- [ ] **Step 3: Add round-trip test**

```rust
#[test]
fn test_config_roundtrip_flat_json() {
    // Load existing config.json format
    let json = r#"{"retrieval_mode":"keyword","retrieval_backend":"fts"}"#;
    let config: Config = serde_json::from_str(json).unwrap();
    assert_eq!(config.retrieval.mode, "keyword");

    // Re-serialize and verify flat structure preserved
    let reserialized = serde_json::to_string(&config).unwrap();
    assert!(reserialized.contains("\"retrieval_mode\""));
}
```

- [ ] **Step 4: Create remaining sub-structs**

Repeat for: `ContextInjectionConfig`, `StewardshipConfig`, `BuildHygieneConfig`, `ToolExecutionConfig`, `AgentConfig`.

**NOTE:** The `#[serde(flatten)]` approach requires that field names remain unchanged in the JSON. If Config uses `pub retrieval_mode: String` today, the sub-struct field must also serialize to `"retrieval_mode"`, not `"mode"`. Use `#[serde(rename = "retrieval_mode")]` on the sub-struct fields.

- [ ] **Step 5: Build and test with existing config.json**

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings
```

Also manually test: load a real `.impulse/config.json`, verify all settings round-trip correctly.

---

### Task 12: Extract main.rs CLI Dispatch

**Files:**
- Modify: `impulse-rs/src/main.rs` (1,548 lines → ~500 lines)

- [ ] **Step 1: Identify what's in main.rs**

```bash
cd impulse-rs && grep -n "^fn \|^async fn \|^pub fn " src/main.rs | head -30
```

Identify: CLI arg parsing (clap), match dispatch, and any inline handler code that should live in `handlers/`.

- [ ] **Step 2: Move inline handler bodies to handlers/ modules**

For any CLI command that has >20 lines of inline logic in main.rs, extract to the appropriate handler file.

- [ ] **Step 3: Build and test**

```bash
cd impulse-rs && cargo build && cargo test
```

---

### Task 13: Consolidate Serialization Patterns

**Files:**
- Create: `impulse-rs/src/storage/helpers.rs`
- Modify: Multiple files that duplicate atomic write / JSON parse patterns

- [ ] **Step 1: Create helpers module**

```rust
// src/storage/helpers.rs

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

/// Parse JSON with standardized error context.
pub fn parse_json<T: DeserializeOwned>(s: &str, context: &str) -> Result<T> {
    serde_json::from_str(s).with_context(|| format!("Failed to parse JSON for {}", context))
}

/// Serialize to pretty JSON with standardized error context.
pub fn to_json_pretty<T: Serialize>(data: &T) -> Result<String> {
    serde_json::to_string_pretty(data).context("Failed to serialize to JSON")
}
```

- [ ] **Step 2: Replace highest-frequency duplicate sites**

Start with the 10 most repeated patterns. Don't try to replace all 200+ sites — focus on the ones that are identical copy-paste.

- [ ] **Step 3: Build and test**

```bash
cd impulse-rs && cargo build && cargo test
```

---

### Task 14: Extract Integration Test Infrastructure

**Files:**
- Modify: `impulse-rs/src/integration_tests.rs` (2,371 lines)

- [ ] **Step 1: Identify shared test helpers**

```bash
cd impulse-rs && grep -n "^fn \|^pub fn \|^async fn " src/integration_tests.rs | head -15
```

Identify: `run_impulse()`, `run_impulse_with_impulse_dir()`, `run_impulse_with_env()`, `start_daemon()`, `seed_retrieval_history()`.

- [ ] **Step 2: Extract to a test helpers module**

Move shared helpers to a common location within the integration test file (grouped at top), or if the file structure supports it, into a `test_helpers` module. Since these are `#[cfg(test)]` code, they stay in the test module but can be deduplicated.

- [ ] **Step 3: Consolidate duplicate test setup**

Look for tests that repeat the same 10-line setup pattern and extract to a helper.

- [ ] **Step 4: Build and test**

```bash
cd impulse-rs && cargo test -- integration_tests 2>&1 | tail -10
```

---

### Task 15: Phase 2 Commit

- [ ] **Step 1: Full verification**

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

- [ ] **Step 2: Measure LOC delta**

```bash
cd impulse-rs && find . -name "*.rs" | xargs wc -l | tail -1
```

- [ ] **Step 3: Verify no file exceeds 800 lines (target)**

```bash
cd impulse-rs && find . -name "*.rs" -path "*/src/*" -exec wc -l {} + | sort -rn | awk '$1 > 800 {print}'
```

- [ ] **Step 4: Stage and commit**

---

## PHASE 3: Agent Harness Wiring

### Task 16: Planning Checkpoint

- [ ] **Step 1: Updated metrics**
- [ ] **Step 2: Review agent module state after Phase 1+2 changes**
- [ ] **Step 3: Confirm data flow: extractor → insights → agent → coordinator → IPC**

---

### Task 17: Wire Context Lifecycle Insights into Agent Prompts

This is the **core "closing the loop"** task. Currently the extractor produces `ExtractedInsight` but the agent builds prompts ignoring them.

**Files:**
- Modify: `impulse-rs/src/agent/prompts.rs` — add `build_context_prompt()`
- Modify: `impulse-rs/src/agent/mod.rs` — add `query_with_context()`
- Modify: `impulse-rs/src/daemon/mod.rs` (or handlers.rs) — pass insights to agent

- [ ] **Step 1: Write failing test for context prompt builder**

```rust
// In agent/prompts.rs tests
#[test]
fn test_build_context_prompt_includes_insights() {
    use crate::context_lifecycle::types::{AgentKind, ExtractedInsight, InsightType};
    use chrono::Utc;

    let insights = vec![
        ExtractedInsight {
            pane_id: 1,
            agent_kind: AgentKind::ClaudeCode,
            timestamp: Utc::now(),
            insight_type: InsightType::FileModified,
            content: "src/main.rs".to_string(),
            intent: None,
        },
        ExtractedInsight {
            pane_id: 2,
            agent_kind: AgentKind::OpenCode,
            timestamp: Utc::now(),
            insight_type: InsightType::ErrorEncountered,
            content: "cannot find module".to_string(),
            intent: None,
        },
    ];

    let prompt = build_context_prompt(&insights);
    assert!(prompt.contains("src/main.rs"));
    assert!(prompt.contains("cannot find module"));
    assert!(prompt.contains("file_modified"));
    assert!(prompt.contains("error_encountered"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd impulse-rs && cargo test test_build_context_prompt_includes_insights -- --nocapture 2>&1 | tail -5
```

Expected: FAIL — `build_context_prompt` doesn't exist yet.

- [ ] **Step 3: Implement build_context_prompt**

```rust
// In agent/prompts.rs

use crate::context_lifecycle::types::ExtractedInsight;

/// Build a structured context block from extracted insights for agent prompts.
/// Groups insights by type and formats them as a readable summary.
pub fn build_context_prompt(insights: &[ExtractedInsight]) -> String {
    if insights.is_empty() {
        return String::new();
    }

    let mut sections = Vec::new();
    sections.push("## Cross-Pane Context\n".to_string());

    // Group by insight type
    let mut by_type: std::collections::BTreeMap<&str, Vec<&ExtractedInsight>> =
        std::collections::BTreeMap::new();
    for insight in insights {
        by_type
            .entry(insight.insight_type.as_str())
            .or_default()
            .push(insight);
    }

    for (insight_type, items) in &by_type {
        sections.push(format!("### {}", insight_type));
        for item in items {
            let intent_str = item
                .intent
                .map(|i| format!(" [{}]", i.as_str()))
                .unwrap_or_default();
            sections.push(format!(
                "- pane-{} ({}){}: {}",
                item.pane_id,
                item.agent_kind.label(),
                intent_str,
                item.content
            ));
        }
        sections.push(String::new());
    }

    sections.join("\n")
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cd impulse-rs && cargo test test_build_context_prompt_includes_insights -- --nocapture 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Add query_with_context to ImpulseAgent**

```rust
// In agent/mod.rs, add to impl ImpulseAgent:

    /// Query with extracted insights injected as context.
    /// Builds a context block from insights and prepends it to the user prompt.
    pub async fn query_with_context(
        &mut self,
        system_prompt: &str,
        user_prompt: &str,
        insights: &[ExtractedInsight],
    ) -> AgentResult<String> {
        let context_block = prompts::build_context_prompt(insights);
        let full_prompt = if context_block.is_empty() {
            user_prompt.to_string()
        } else {
            format!("{}\n\n---\n\n{}", context_block, user_prompt)
        };
        self.query(system_prompt, &full_prompt).await
    }
```

- [ ] **Step 6: Update daemon handler to pass insights**

In `handle_agent_request()`, after loading the agent config, also load the active pane context states and pass their insights to `query_with_context()` instead of `query()`.

- [ ] **Step 7: Build and test**

```bash
cd impulse-rs && cargo build && cargo test 2>&1 | tail -5
```

---

### Task 18: Activate Intent Classification in Extractor

**Files:**
- Modify: `impulse-rs/src/context_lifecycle/extractor.rs` — populate `insight.intent`
- Modify: `impulse-rs/src/agent/coordinator.rs` — use intent in recommendations

- [ ] **Step 1: Write failing test for intent population**

```rust
#[test]
fn test_extracted_insight_has_intent() {
    // Create an insight from content that clearly indicates testing
    let content = "Running cargo test --all";
    let intent = IntentCategory::from_keywords(&[content]);
    assert_eq!(intent, IntentCategory::Testing);
}
```

- [ ] **Step 2: Find all insight creation sites in extractor.rs**

```bash
cd impulse-rs && grep -n "ExtractedInsight {" src/context_lifecycle/extractor.rs
```

- [ ] **Step 3: At each creation site, populate intent**

Replace `intent: None` with:
```rust
intent: Some(IntentCategory::from_keywords(&[&content])),
```

- [ ] **Step 4: Update coordinator to use intent for prioritization**

In `run_local_coordination()`, after generating recommendations, sort by priority based on insight intents. Deploying intents get higher priority for conflict warnings.

- [ ] **Step 5: Build and test**

```bash
cd impulse-rs && cargo build && cargo test
```

---

### Task 19: Wire Coordinator Production Paths

**Files:**
- Modify: `impulse-rs/src/agent/coordinator.rs:358-365` — `run_local_coordination()` already calls all three! Verify this.

- [ ] **Step 1: Verify run_local_coordination already calls all detectors**

Reading coordinator.rs:358-365, `run_local_coordination()` already calls:
1. `detect_file_conflicts(insights)` ✓
2. `detect_cross_pane_errors(insights)` ✓
3. `detect_delegation_events(insights)` ✓

This is already wired! The earlier research was incorrect — the coordinator IS calling cross-pane errors.

**Verify:**
```bash
cd impulse-rs && sed -n '358,365p' src/agent/coordinator.rs
```

- [ ] **Step 2: If already wired, verify with test**

Run the existing coordinator tests to confirm all paths are exercised:
```bash
cd impulse-rs && cargo test coordinator -- --nocapture
```

- [ ] **Step 3: Wire aggregate_pane_summaries into coordinate_llm path**

`aggregate_pane_summaries()` is the one that's NOT called in production. Add it to the daemon's coordination flow so pane summaries are available when the LLM-based coordination is triggered.

---

### Task 20: Wire ConflictResolver History to IPC

**Files:**
- Modify: `impulse-rs/src/daemon/mod.rs` (or `protocol.rs` + `handlers.rs`)

- [ ] **Step 1: Add new DaemonRequest variants**

```rust
/// Query conflict resolution history
GetConflictHistory,
/// Clear resolved conflicts from tracking
ClearResolvedConflicts,
```

- [ ] **Step 2: Add handler implementations**
- [ ] **Step 3: Add tests for round-trip**
- [ ] **Step 4: Build and test**

---

### Task 21: Upgrade Harness Mode with Structured Protocol

**Files:**
- Modify: `impulse-rs/src/agent/mod.rs` — `harness_query()` method

- [ ] **Step 1: Define structured request/response types**

```rust
#[derive(Serialize)]
struct HarnessRequest {
    system_prompt: String,
    user_prompt: String,
    context_insights: Vec<InsightSummary>,
    max_tokens: u32,
}

#[derive(Deserialize)]
struct HarnessResponse {
    content: String,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Serialize)]
struct InsightSummary {
    pane_id: usize,
    insight_type: String,
    content: String,
}
```

- [ ] **Step 2: Try structured mode first, fallback to --print**

In `harness_query()`:
```rust
// Try structured mode via stdin
let result = self.harness_query_structured(&request).await;
match result {
    Ok(response) => Ok(response.content),
    Err(_) => {
        // Fallback to simple --print mode
        self.harness_query_simple(&combined_prompt).await
    }
}
```

- [ ] **Step 3: Test both paths**
- [ ] **Step 4: Build and test**

---

### Task 22: Add Agent Session Awareness

**Files:**
- Modify: `impulse-rs/src/agent/mod.rs` — add session history to ImpulseAgent

- [ ] **Step 1: Add session_history field**

```rust
pub struct ImpulseAgent {
    config: ImpulseAgentConfig,
    inner: Option<Agent>,
    recommendations: Vec<Recommendation>,
    /// Recent query/response pairs for session continuity (bounded to 5).
    session_history: Vec<(String, String)>,
}
```

- [ ] **Step 2: Include history in prompt construction**
- [ ] **Step 3: Add clear_session() method**
- [ ] **Step 4: Test multi-turn continuity**
- [ ] **Step 5: Build and test**

---

### Task 23: Wire Specialized Methods to Daemon IPC

**Files:**
- Modify: `impulse-rs/src/agent/mod.rs` — remove `#[cfg(test)]` gates from Task 2
- Modify: `impulse-rs/src/daemon/mod.rs` (or handlers.rs) — add IPC endpoints

- [ ] **Step 1: Remove #[cfg(test)] gates added in Task 2**

Re-enable `review_code()`, `analyze_error()`, `coordinate_llm()`, `summarize_pane()`.

- [ ] **Step 2: Add DaemonRequest variants**

```rust
AgentReviewCode { file_path: String, diff: String },
AgentAnalyzeError { error_text: String, context: Option<String> },
AgentSummarizePane { pane_id: usize },
```

- [ ] **Step 3: Implement handlers**
- [ ] **Step 4: Test each endpoint**
- [ ] **Step 5: Build and test**

---

### Task 24: Phase 3 Commit

- [ ] **Step 1: Full verification**

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

- [ ] **Step 2: Stage and commit**

---

## PHASE 4: Verification & Alignment

### Task 25: Planning Checkpoint — Full Metrics Audit

- [ ] **Step 1: LOC comparison**

```bash
cd impulse-rs && find . -name "*.rs" | xargs wc -l | tail -1
```

Compare: baseline 132,442 → Phase 1 → Phase 2 → Phase 3 → current.

- [ ] **Step 2: Test count comparison**

```bash
cd impulse-rs && cargo test 2>&1 | grep "test result"
```

- [ ] **Step 3: Agent feature matrix**

| Feature | Before | After |
|---------|--------|-------|
| Context → prompts | Disconnected | Wired (Task 17) |
| Intent classification | Always None | Populated (Task 18) |
| Cross-pane errors | In coordinator only | In production (Task 19) |
| Conflict history | Tracked, not queryable | IPC endpoint (Task 20) |
| Structured harness | String only | JSON + fallback (Task 21) |
| Session awareness | Stateless | 5-turn history (Task 22) |
| Specialized endpoints | Generic only | 3 new IPC types (Task 23) |

---

### Task 26: Agent Harness Tests (PARALLEL with Task 27)

- [ ] **Step 1: Test context → prompt pipeline**
- [ ] **Step 2: Test intent classification accuracy**
- [ ] **Step 3: Test coordinator full pipeline**
- [ ] **Step 4: Test conflict history round-trip**
- [ ] **Step 5: Test structured harness fallback**
- [ ] **Step 6: Test session awareness**
- [ ] **Step 7: Test specialized IPC endpoints**

Target: 15-25 new tests.

---

### Task 27: Tooling Module Tests (PARALLEL with Task 26)

**Files:**
- Modify: `impulse-rs/src/tooling/` test modules

- [ ] **Step 1: Test capability enforcement — denied tool blocked**
- [ ] **Step 2: Test param validation — invalid params rejected**
- [ ] **Step 3: Test tool registration — successful invocation**
- [ ] **Step 4: Test schema export accuracy**

Target: 20+ new tests for the security-critical tooling module.

---

### Task 28: Full Workspace Verification

- [ ] **Step 1: Full build**
```bash
cd impulse-rs && cargo build --all-features
```
- [ ] **Step 2: Full test**
```bash
cd impulse-rs && cargo test
```
- [ ] **Step 3: Clippy clean**
```bash
cd impulse-rs && cargo clippy --all-features --all-targets -- -D warnings
```
- [ ] **Step 4: Fmt clean**
```bash
cd impulse-rs && cargo fmt --check
```

---

### Task 29: Documentation Updates

- [ ] **Step 1: Update CLAUDE.md** — new module counts, LOC, architecture changes
- [ ] **Step 2: Update ROADMAP-PLAN.md** — mark completed items
- [ ] **Step 3: Update MEMORY.md** — record plan outcomes
- [ ] **Step 4: Commit docs**

---

### Task 30: Final Metrics & Plan Completion

- [ ] **Step 1: Final LOC count**
- [ ] **Step 2: Final test count**
- [ ] **Step 3: Files >800 lines audit**
- [ ] **Step 4: #[allow(dead_code)] count**
- [ ] **Step 5: Agent feature matrix — all 7 features wired**
- [ ] **Step 6: Record in ralph-plan-3.md Working Log**
