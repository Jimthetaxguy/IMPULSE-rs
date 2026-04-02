# Tooling Improvement Spec — 2026 Q2

> Status: Draft v2  
> Author: OpenCode (synthesized from Claude Code leak analysis + Rust harness research + existing Impulse architecture + Meta-Harness/Rust-Multi-Agent/WASM docs)  
> Date: 2026-04-01  
> Valid until: Superseded by user feedback

---

## 1. Executive Summary

The Claude Code leak (March 31, 2026) confirms that Impulse's existing architecture is **70% of a Claude Code harness**. The gap is not foundational — it's the **outer evaluation loop** and the explicit plumbing connecting existing components into a production harness. The Meta-Harness and Rust-Multi-Agent docs already describe the target architecture. This spec bridges the gap.

### What's Confirmed by the Leak

| Claude Code Component | Impulse Equivalent | Status |
|---|---|---|
| `query()` agent loop | `src/orchestration/mod.rs` | Exists — static routing, not yet a policy artifact |
| CLAUDE.md loading + context assembly | `src/injection/engine.rs` | Exists — injection policy surface |
| `HISTORY.jsonl` trace archive | `.impulse/HISTORY.jsonl` | ✅ Already append-only |
| `GENOME.md` memory | `.impulse/GENOME.md` | ✅ Already present |
| `retrieval.db` | `retrieval.db` | ✅ Already indexed |
| **Prompt cache boundary** | Not yet explicit | **Gap — highest leverage** |
| **`HarnessRecord` + run ID** | Not yet formalized | **Gap** |
| **Evaluation runner** | Not yet present | **Gap** |
| **AutoDream distillation** | Not yet present | **Gap** |

### The Three High-Leverage Patterns

1. **Prompt Cache Boundary as First-Class Artifact** — The highest-leverage harness optimization. Claude Code tracks 14 cache-break vectors because a cache miss on Opus is $0.15/token at scale. Impulse's injection engine assembles context — formalize the `stable` vs `dynamic` split and memoize accordingly.

2. **Bounded Concurrency via Semaphore** — Claude Code's unbounded AutoCompact retry burned 250K API calls/day. The fix is `MAX_CONSECUTIVE_FAILURES = 3` — but in Rust, enforce it structurally with `Semaphore` at the concurrency primitive level, not via a retry counter.

3. **Versioned\<T\> + CAS** — Parallel agent writes to shared state silently overwrite each other in TypeScript. In Rust, `Versioned<T>` with compare-and-swap makes silent data races into loud compile errors.

---

## 2. Architecture: Where Impulse Already Is

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Impulse (owned)                                  │
│                                                                      │
│  ┌─────────────┐  ┌──────────────┐  ┌─────────────────────────┐   │
│  │ Memory      │  │ KAIROS       │  │ Hook System            │   │
│  │ (3-layer + │  │ Daemon       │  │ (guardrails +         │   │
│  │  AutoDream)│  │ (webhooks +  │  │  tracking)             │   │
│  │             │  │  proactiv)   │  │                        │   │
│  └─────────────┘  └──────────────┘  └─────────────────────────┘   │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │ Tool Registry (DynamicTool trait + capability management)         │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │ IPC Layer (Unix socket, daemon ↔ TTY communication)             │ │
│  └────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
                              │ delegate
                              ▼
        ┌─────────────────────────────────────────────────────┐
        │  claude_agent_sdk    (Claude Code CLI harness)    │
        │  claude-agent         (Anthropic API direct)      │
        │  rig                  (Multi-provider framework)    │
        └─────────────────────────────────────────────────────┘

WHAT IS MISSING:
┌────────────────────────────────────────────────────────────────┐
│  Missing from Impulse Today                                     │
│                                                                │
│  1. PolicySnapshot + EvaluationTrace + HarnessRecord (types)  │
│  2. Bounded evaluation runner (Semaphore, not retry counter)   │
│  3. Prompt cache boundary (stable vs dynamic ContextAssembly) │
│  4. Versioned<T> + CAS for orchestration state                │
│  5. AutoDream background task                                  │
│  6. HarnessRecord audit trail linking policy→eval→score       │
└────────────────────────────────────────────────────────────────┘
```

---

## 3. The Three Missing Rust Types

These are the concrete types that close the 30% gap. Add to `impulse-rs/src/harness/`:

### 3.1 `HarnessRecord` — Policy Snapshot ↔ Evaluation Output Link

```rust
// impulse-rs/src/harness/record.rs

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessRecord {
    /// Correlation key for everything in this evaluation run
    pub run_id: Uuid,
    /// Which policy version was active when this run was scored
    pub policy_snapshot_path: PathBuf,
    /// Final score (0.0 – 1.0)
    pub score: f32,
    /// Path to the raw EvaluationTrace on disk
    pub trace_path: PathBuf,
    pub created_at: DateTime<Utc>,
    /// If this run was dominated by another, the dominating run_id
    /// Used for Pareto front tracking
    pub dominated_by: Option<Uuid>,
    /// Key metrics from this run
    pub metrics: RunMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetrics {
    pub turns: usize,
    pub tool_calls: usize,
    pub context_tokens_used: usize,
    pub cache_hit_ratio: f32,
    pub total_duration_ms: u64,
    pub consecutive_failures: u8,
}
```

### 3.2 `EvaluationTrace` — The Raw Causal Path

```rust
// impulse-rs/src/harness/trace.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationTrace {
    pub run_id: Uuid,
    pub turns: Vec<TracedTurn>,
    pub tool_calls: Vec<TracedToolCall>,
    pub context_tokens_used: usize,
    pub cache_hit_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracedTurn {
    pub turn_id: usize,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub model: String,
    pub cache_created: bool,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracedToolCall {
    pub tool_name: String,
    pub concurrent_batch: bool,
    pub success: bool,
    pub duration_ms: u64,
    pub input_tokens: usize,
}
```

### 3.3 `PolicySnapshot` — Versioned Injection/Routing Policy

```rust
// impulse-rs/src/harness/policy.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySnapshot {
    pub version: u32,
    pub routing_rules: Vec<RoutingRule>,
    pub injection_config: InjectionConfig,
    /// Context budget in tokens — used to compute cache efficiency
    pub context_budget_tokens: usize,
    pub created_at: DateTime<Utc>,
    pub parent_version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub pattern: String,  // glob or regex
    pub capability: Capability,
    pub handler: String,  // module path
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionConfig {
    pub max_history_turns: usize,
    pub max_context_tokens: usize,
    pub cache_boundary_enabled: bool,
}
```

### Integration: `Versioned<T>` for Safe Parallel Writes

```rust
// impulse-rs/src/state/versioned.rs

use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug)]
pub struct Versioned<T> {
    value: T,
    version: AtomicU32,
}

impl<T: Clone> Versioned<T> {
    pub fn new(value: T) -> Self {
        Self { value, version: AtomicU32::new(0) }
    }

    /// Compare-and-swap. Fails if expected_version doesn't match current.
    /// Returns Ok((new_value, new_version)) on success.
    pub fn compare_exchange(
        &self,
        expected_version: u32,
        new_value: T,
    ) -> Result<(T, u32), (T, u32)> {
        let current = self.version.load(Ordering::SeqCst);
        if current != expected_version {
            return Err((self.value.clone(), current));
        }

        let new_version = current + 1;
        self.value = new_value.clone();
        self.version.store(new_version, Ordering::SeqCst);
        Ok((new_value, new_version))
    }

    pub fn get(&self) -> (T, u32) {
        (self.value.clone(), self.version.load(Ordering::SeqCst))
    }
}

// Usage in orchestration:
let routing_state = Arc::new(Versioned::new(RoutingPolicy::default()));

async fn update_routing(policy: RoutingPolicy) -> Result<()> {
    let (_, ver) = routing_state.get();
    routing_state.compare_exchange(ver, policy)
        .map_err(|_| HarnessError::RoutingPolicyChanged)?;
    Ok(())
}
```

---

## 4. Prompt Cache Boundary: The Highest-Leverage Harness Optimization

### 4.1 Why This Matters

Claude Code tracks 14 distinct cache-break vectors. At Opus pricing ($0.015/1K input tokens cached, $0.075/1K uncached), the difference between a cache hit and miss on a 32K-token system prompt is ~$2.40/turn. At 100 turns/session, that's $240/session. For a harness running 1,000 sessions/day, the cache hit rate is an accounting problem.

### 4.2 The Split: `ContextAssembly`

```rust
// impulse-rs/src/injection/context_assembly.rs

/// ContextAssembly formalizes the cache boundary.
/// Anything in `stable` is memoized once per session and sent with
/// cache_control: ephemeral (write-to-cache).
/// Anything in `dynamic` is always fresh, no caching.
#[derive(Debug)]
pub struct ContextAssembly {
    pub stable: Vec<NamedSection>,
    pub dynamic: Vec<NamedSection>,
}

#[derive(Debug, Clone)]
pub struct NamedSection {
    pub name: &'static str,
    pub content: String,
}

/// Stable sections — computed once, memoized, byte-identical per turn.
/// These are what get cache_control: ephemeral in the API request.
impl ContextAssembly {
    pub fn to_api_messages(&self) -> Vec<ApiMessage> {
        let mut msgs = vec![];

        // Stable sections: wrapped with cache_control
        for section in &self.stable {
            msgs.push(ApiMessage::text_with_cache(
                &section.content,
                CacheControl::Ephemeral,
            ));
        }

        // Dynamic sections: no cache control — always fresh
        for section in &self.dynamic {
            msgs.push(ApiMessage::text(&section.content));
        }

        msgs
    }
}
```

### 4.3 Stable vs Dynamic Sections

| Content | Type | Why |
|---------|------|-----|
| System prompt template | `stable` | Never changes per session |
| Capability definitions | `stable` | Only changes when policy changes |
| Routing rules | `stable` | Changes infrequently (versioned) |
| CWD | `dynamic` | Changes every turn |
| Git status | `dynamic` | Changes every turn |
| Session ID | `dynamic` | New every session |
| Timestamp (absolute) | `dynamic` | Changes every request |
| Last run summary | `dynamic` | Changes between turns |

### 4.4 The 14 Cache-Break Vectors (from `promptCacheBreakDetection.ts`)

Anything that makes a stable section non-byte-identical across turns:

```rust
/// Common cache busters — all must be in `dynamic` sections:
/// - Timestamps with seconds precision → round to minute or use absolute date only
/// - PIDs in prompts → never include process::id()
/// - Run IDs as Uuid::new_v4() → use OnceLock, computed once per session
/// - Relative dates ("2 hours ago") → convert to absolute on first write
/// - File paths with temp prefixes (/tmp/impulse-abc123/) → strip before cache
/// - Debug output ( println! in prompt) → strip from stable sections

static SESSION_START: Lazy<DateTime<Utc>> = Lazy::new(Utc::now);
static SESSION_ID: OnceLock<String> = OnceLock::new();

fn session_id() -> &'static str {
    SESSION_ID.get_or_init(|| Uuid::new_v4().to_string())
}

fn stable_system_prompt() -> String {
    // This is cached — must be byte-identical every turn
    include_str!("../prompts/system_static.md").to_string()
}

fn dynamic_context(state: &DaemonState) -> String {
    format!(
        "session: {}\ntime: {}\ncwd: {}\n",
        session_id(),
        SESSION_START.format("%Y-%m-%dT%H:%M:%SZ"),  // stable format, not now()
        state.cwd().display(),
    )
}
```

### 4.5 Sticky Latch Pattern

Claude Code uses "sticky latches" — once a cache boundary section changes, it stays changed for the remainder of the session (even if it would otherwise revert). Model this explicitly:

```rust
struct CacheLatch {
    dirty_sections: HashSet<&'static str>,
    session_once_sections: HashSet<&'static str>,
}

impl CacheLatch {
    /// Call when a stable section changes mid-session.
    /// After this, that section stays in `dynamic` for the rest of the session.
    fn mark_sticky(&mut self, section: &'static str) {
        self.dirty_sections.insert(section);
        self.session_once_sections.insert(section);
    }

    fn is_sticky(&self, section: &'static str) -> bool {
        self.session_once_sections.contains(section)
    }
}
```

---

## 5. Bounded Concurrency: Semaphore, Not Retry Counter

Claude Code's AutoCompact failure cascade (250K API calls/day) happened because a retry counter had no upper bound. In Rust, the right model is `Semaphore` — enforce the bound at the concurrency primitive level.

### 5.1 The Pattern

```rust
// impulse-rs/src/harness/bounded_runner.rs

use tokio::sync::{Semaphore, SemaphorePermit};
use std::sync::Arc;

const MAX_CONCURRENT_AUTONOMOUS_OPS: usize = 4;

pub struct BoundedRunner {
    semaphore: Arc<Semaphore>,
}

impl BoundedRunner {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// Acquire a permit. If the semaphore is exhausted, this waits.
    /// The permit is dropped automatically when the future completes.
    pub async fn run<F, Fut>(&self, fut: F) -> Result<F::Output, RunError>
    where
        F: Future,
    {
        let _permit = self.semaphore.acquire().await?;

        // Track consecutive failures structurally, not via counter
        let result = fut.await;

        Ok(result)
    }

    /// Try to acquire without waiting.
    pub fn try_run<F, Fut>(&self, fut: F) -> Option<Result<F::Output, RunError>>
    where
        F: Future,
    {
        // Non-blocking — returns None if semaphore is exhausted
        let permit = self.semaphore.try_acquire().ok()?;
        Some(fut.await)
    }
}
```

### 5.2 `AgentLoopState` — Structurally Enforced Retry Cap

```rust
// impulse-rs/src/harness/loop_state.rs

pub enum AgentLoopState {
    Running { consecutive_failures: u8 },
    Compacting,
    Done,
    Failed(HarnessError),
}

const MAX_CONSECUTIVE_FAILURES: u8 = 3;

impl AgentLoopState {
    pub fn record_failure(self) -> Self {
        match self {
            // At 2 failures (0, 1, 2), the NEXT failure triggers escalation.
            // This is structurally enforced: there's no code path that allows
            // consecutive_failures to reach 3 without transitioning to Failed.
            Self::Running { consecutive_failures } if consecutive_failures >= 2 => {
                Self::Failed(HarnessError::CompactionFailureEscalation {
                    failures: consecutive_failures + 1,
                })
            }
            Self::Running { consecutive_failures } => {
                Self::Running { consecutive_failures: consecutive_failures + 1 }
            }
            other => other,
        }
    }

    pub fn record_success(&mut self) {
        *self = match std::mem::replace(self, Self::Done) {
            Self::Running { .. } => Self::Running { consecutive_failures: 0 },
            other => other,
        };
    }
}
```

---

## 6. Rust Type-Level Enforcement vs TypeScript Runtime

This is the structural advantage Rust gives over Claude Code's 23–42 regex-based bash security checks.

### 6.1 Tool Safety as a Trait

```rust
// impulse-rs/src/tooling/trait.rs

/// AgentTool is implemented by anything the agent can call.
/// The trait bounds (Send + Sync) enforce that tools are safe to use
/// across concurrent task execution — the borrow checker catches
/// non-thread-safe tools at compile time, not at 2AM in production.
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &'static str;

    /// Compile-time default: tools are NOT concurrent-safe.
    /// A tool that claims is_concurrent_safe() but mutates shared state
    /// will fail the borrow checker, not fail in production.
    fn is_concurrent_safe(&self) -> bool { false }

    /// If true, this tool is read-only — safe to run in parallel with anything.
    fn is_read_only(&self) -> bool { false }

    async fn execute(&self, ctx: &ToolContext, input: serde_json::Value)
        -> Result<serde_json::Value, ToolError>;
}

/// Read-only tools can run concurrently with everything.
/// Write tools claim is_concurrent_safe only if they handle their own internal
/// serialization.
pub struct ReadTool<T: AgentTool> { inner: T }
pub struct WriteTool { /* ... */ }

impl<T: AgentTool> AgentTool for ReadTool<T> {
    fn is_read_only(&self) -> bool { true }
    // ... delegates execute to inner
}
```

### 6.2 Capability-Based Access as Rust Enum

```rust
// impulse-rs/src/tooling/capability.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    ReadFiles,
    WriteFiles,
    WriteScopedFiles { path_prefix: &'static str },
    ExecuteBash,
    ExecuteBashRestricted { allowed_commands: &'static [&'static str] },
    SpawnSubagent,
    ModifyState,
}

pub struct ToolRegistry {
    tools: HashMap<&'static str, Arc<dyn AgentTool>>,
    capability_map: HashMap<Capability, Vec<&'static str>>,
}

impl ToolRegistry {
    /// Check if a given session/capability level can call this tool.
    /// Returns Err if the tool requires capabilities the session doesn't have.
    /// This is a compile-time-style check — no regex, no fuzzy matching.
    pub fn check(&self, session: &SessionCaps, tool: &str) -> Result<(), CapabilityError> {
        let Some(tool_def) = self.tools.get(tool) else {
            return Err(CapabilityError::ToolNotFound(tool));
        };

        let required = self.tool_requirements(tool);

        for cap in required {
            if !session.capabilities.contains(&cap) {
                return Err(CapabilityError::InsufficientCapability {
                    required: cap,
                    session_caps: session.capabilities.clone(),
                });
            }
        }

        Ok(())
    }
}
```

### 6.3 Tool Batching with `JoinSet`

```rust
// impulse-rs/src/tooling/batch.rs

use tokio::task::JoinSet;
use std::sync::Arc;

pub async fn execute_tool_batch(
    tools: Vec<(Arc<dyn AgentTool>, serde_json::Value)>,
) -> Vec<serde_json::Value> {
    // Partition: all concurrent-safe tools batch together.
    // The FIRST exclusive tool terminates the batch (siblingAbort behavior).
    let all_concurrent = tools.iter()
        .all(|(t, _)| t.is_concurrent_safe());

    if all_concurrent {
        let mut set = JoinSet::new();
        for (tool, input) in tools {
            let tool_name = tool.name().to_string();
            set.spawn(async move {
                tool.execute(/* ... */, input).await
            });
        }

        let mut results = vec![];
        while let Some(r) = set.join_next().await {
            results.push(r.unwrap_or_else(|e| json!({"error": e.to_string() })));
        }
        results
    } else {
        // Serial execution — exclusive tool, run alone
        let mut results = vec![];
        for (tool, input) in tools {
            let r = tool.execute(/* ... */, input).await;
            results.push(r.unwrap_or_else(|e| json!({"error": e.to_string() })));
            // Sibling abort: stop on first failure
            if results.last().unwrap().contains_key("error") {
                break;
            }
        }
        results
    }
}
```

---

## 7. Rust Harness Loop: The Complete Pattern

```rust
// impulse-rs/src/harness/mod.rs

use futures::StreamExt;
use tokio::sync::Semaphore;
use std::sync::Arc;

const MAX_CONSECUTIVE_FAILURES: u8 = 3;
const MAX_CONCURRENT_EVALS: usize = 4;

pub struct Harness {
    runner: BoundedRunner,
    state: Arc<Versioned<HarnessState>>,
    tool_registry: Arc<ToolRegistry>,
    context_assembly: ContextAssembly,
    evaluation_trace: EvaluationTrace,
}

pub enum HarnessStep {
    Continue,
    Done,
    Compact,
}

impl Harness {
    pub async fn run(&mut self, prompt: &str) -> Result<HarnessResult, HarnessError> {
        let mut consecutive_failures = 0u8;
        let mut loop_state = AgentLoopState::Running { consecutive_failures };

        loop {
            match self.step(prompt, &mut loop_state).await {
                Ok(HarnessStep::Continue) => {
                    consecutive_failures = 0;
                }
                Ok(HarnessStep::Done) => {
                    return Ok(self.finalize());
                }
                Ok(HarnessStep::Compact) => {
                    loop_state = AgentLoopState::Compacting;
                    self.compact().await?;
                    loop_state = AgentLoopState::Running { consecutive_failures };
                }
                Err(HarnessError::ContextOverflow) => {
                    consecutive_failures += 1;
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        return Err(HarnessError::CompactionFailureEscalation {
                            failures: consecutive_failures,
                        });
                    }
                    loop_state = AgentLoopState::Compacting;
                    self.compact().await?;
                    loop_state = AgentLoopState::Running { consecutive_failures };
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn step(&mut self, prompt: &str, state: &mut AgentLoopState)
        -> Result<HarnessStep, HarnessError>
    {
        let permit = self.runner.semaphore.acquire().await?;

        let messages = self.context_assembly.to_api_messages(prompt);

        let stream = claude_agent_sdk::query_stream(prompt, messages, /* opts */);
        let mut stream = stream.await?;

        while let Some(result) = stream.next().await {
            match result? {
                claude_agent_sdk::Message::Assistant(msg) => {
                    self.evaluation_trace.record_turn(&msg);
                    // ... render to TTY
                }
                claude_agent_sdk::Message::ToolUse(call) => {
                    let batch_results = execute_tool_batch(self.build_tool_batch(&call)).await;
                    for (tool_name, result) in batch_results {
                        self.evaluation_trace.record_tool_call(&tool_name, &result);
                        stream.feed_result(tool_name, result).await?;
                    }
                }
                claude_agent_sdk::Message::Result(_) => break,
                _ => {}
            }
        }

        Ok(HarnessStep::Continue)
    }

    async fn compact(&mut self) -> Result<(), HarnessError> {
        // Compress context, preserve key decisions
        self.context_assembly.compact().await?;
        Ok(())
    }

    fn finalize(&self) -> HarnessResult {
        let record = HarnessRecord {
            run_id: Uuid::new_v4(),
            policy_snapshot_path: self.state.get().0.policy_path.clone(),
            score: self.compute_score(),
            trace_path: self.save_trace().unwrap(),
            created_at: Utc::now(),
            dominated_by: None,
            metrics: self.evaluation_trace.summarize(),
        };

        HarnessResult { record, trace: self.evaluation_trace.clone() }
    }
}
```

---

## 8. WASM Tier Integration (Capability as WIT)

### 8.1 WIT Capability Tiers

If Impulse ever hosts WASM-compiled agent code, the WASM Component Model enforces capability tiers at bytecode validation — before any instruction executes:

| Tier | Agent Type | WIT World | Capabilities |
|------|-----------|-----------|-------------|
| 0 | LLM-only inference | `wasi:llm/inference` | `call-llm`, `log-output` |
| 1 | Read-only analysis | Tier 0 + `wasi:filesystem` (ro) | `read-document`, `query-data` |
| 2 | Analysis + write findings | Tier 1 + `wasi:filesystem` (rw) | `write-findings`, `send-notifications` |
| 3 | Full orchestrator | Tier 2 + `wasi:process` | `spawn-agent`, `manage-workflow` |

### 8.2 WIT Enforcement vs Runtime Checks

```rust
// Impulse as WASM host — enforcement happens at module instantiation:

impl WasmHost {
    pub fn instantiate_with_tier(
        &self,
        wasm_bytes: &[u8],
        tier: CapabilityTier,
    ) -> Result<WasmInstance, WasmError> {
        let engine = &self.engine;
        let store = Store::new(engine, tier);

        // wasm-tools validates the module's WIT requirements against
        // the provided world BEFORE instantiation.
        // An agent compiled with Tier 3 requirements cannot be instantiated
        // in a Tier 1 world — it fails at validation, not at tool-call time.
        let module = Module::new(engine, wasm_bytes)?;
        let instance = Linker::instantiate(&mut store, &module)?;

        Ok(WasmInstance { store, instance })
    }
}
```

This is structurally superior to Claude Code's permission system: a TypeScript runtime check can be bypassed by a compromised JS runtime; a WIT validation failure blocks the module from loading at all.

---

## 9. AutoDream: Production Memory Distillation

### 9.1 The 4-Phase Cycle

| Phase | Input | Output | Mechanism |
|-------|-------|--------|-----------|
| **Orientation** | All `.md` files in `.impulse/` | Knowledge map (entity graph) | `grep` + `once_cell` |
| **Gather Signal** | Knowledge map | High-value: corrections, decisions, patterns | LLM sparse read |
| **Consolidation** | Signals + `HISTORY.jsonl` | Pruned facts, absolute timestamps, stale deleted | `sed` + LLM |
| **Prune & Index** | Consolidated facts | `MEMORY.md` ≤200 lines | Atomic write |

**Trigger** (both required):
- 24+ hours since last run
- 5+ sessions since last run

**Observed**: 913 sessions in ~8 minutes.

### 9.2 State Tracking Additions

```rust
// impulse-rs/src/state/dream_state.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamState {
    pub last_dream_at: Option<DateTime<Utc>>,
    pub sessions_since_dream: u32,
    pub last_dream_run_id: Option<Uuid>,
    pub consecutive_dream_failures: u8,
}

const MAX_CONSECUTIVE_DREAM_FAILURES: u8 = 3;

impl DaemonState {
    pub async fn should_dream(&self) -> Result<bool, StateError> {
        let state = self.dream_state().await?;
        let elapsed = Utc::now() - state.last_dream_at.unwrap_or(epoch);
        Ok(
            elapsed.num_hours() >= 24
            && state.sessions_since_dream >= 5
        )
    }

    pub async fn record_dream_completion(&self, run_id: Uuid) -> Result<(), StateError> {
        self.update_dream_state(DreamState {
            last_dream_at: Some(Utc::now()),
            sessions_since_dream: 0,
            last_dream_run_id: Some(run_id),
            consecutive_dream_failures: 0,
        }).await
    }
}
```

---

## 10. Implementation Priorities

### Phase 1: Quick Wins (1-2 weeks)

| # | Action | Effort | Impact | Deliverable |
|---|--------|--------|--------|-------------|
| 1.1 | Add `--autoConnect` to OpenCode chrome-devtools | 1 line | Medium | Persistent Chrome sessions |
| 1.2 | Add `MAX_CONSECUTIVE_FAILURES` to Impulse daemon loop | 15 lines | High | Prevent runaway loops |
| 1.3 | Formalize `ContextAssembly` with `stable`/`dynamic` split | 1 week | **Highest** | Cache efficiency gains |
| 1.4 | Add `HarnessRecord` + `EvaluationTrace` types | 2 days | High | Policy↔eval linkage |

### Phase 2: Core Memory + Concurrency (2-4 weeks)

| # | Action | Effort | Impact | Deliverable |
|---|--------|--------|--------|-------------|
| 2.1 | Implement `AgentLoopState` with structurally enforced retry cap | 3 days | High | No more unbounded retries |
| 2.2 | Implement `BoundedRunner` with `Semaphore` | 2 days | High | Concurrent eval cap |
| 2.3 | Implement `Versioned<T>` + CAS for orchestration state | 3 days | High | Silent race prevention |
| 2.4 | Implement `ToolConcurrency` trait with compile-time safety | 1 week | Medium | Tool batching safety |
| 2.5 | Implement AutoDream 4-phase in daemon | 1 week | High | `/dream` command |

### Phase 3: KAIROS Scaffolding (4-6 weeks)

| # | Action | Effort | Impact | Deliverable |
|---|--------|--------|--------|-------------|
| 3.1 | Create `impulse-kairos/` crate skeleton | 2 days | Low | New workspace crate |
| 3.2 | Implement webhook receiver | 1 week | High | GitHub event subscription |
| 3.3 | Implement proactive suggestion engine | 1 week | Medium | `suggest` command |
| 3.4 | Add cron-style task scheduler | 3 days | Medium | Background task infrastructure |
| 3.5 | Implement `PolicySnapshot` versioning | 3 days | Medium | Policy audit trail |

### Phase 4: Evaluation + Policy Loop (ongoing)

| # | Action | Effort | Impact | Deliverable |
|---|--------|--------|--------|-------------|
| 4.1 | Implement `HarnessRecord` persistence + Pareto tracking | 1 week | High | Full eval loop |
| 4.2 | Add `PolicySnapshot` serialization + diff | 1 week | Medium | Policy versioning |
| 4.3 | Integrate `claude_agent_sdk` as Claude Code driver | 1 week | Medium | Native Claude Code control |
| 4.4 | Add WIT tier validation to `impulse-wasm` (future) | 2 weeks | Medium | WASM capability enforcement |

---

## 11. Skill Updates Required

| Skill | Update | Priority |
|-------|--------|----------|
| `memory-architecture` | Add AutoDream 4-phase + `/dream` trigger | HIGH |
| `multi-agent-coordination` | Add prompt-as-coordinator + Git worktree section | HIGH |
| `agentic-sdlc` | Add retry cap + 250K API cautionary tale + `AgentLoopState` | HIGH |
| `mcp-tool-development` | Add `claude_agent_sdk` integration guide | MEDIUM |
| `rust-ai-infrastructure` | Add `claude-agent` as native Rust Anthropic SDK | MEDIUM |

---

## 12. Reference: Key Numbers

| Metric | Value |
|--------|-------|
| Claude Code leak date | March 31, 2026 |
| AutoCompact incident | March 10, 2026 |
| Wasted API calls/day | ~250K |
| Max consecutive failures (fix) | 3 |
| Claude Code max tool concurrency | 10 (configurable) |
| Recommended concurrent evals (Impulse) | 4 |
| AutoDream trigger: hours elapsed | 24+ |
| AutoDream trigger: sessions | 5+ |
| Observed sessions/processed | 913 in ~8 min |
| Bash security checks (Claude Code) | 23–42 |
| Feature flags (Claude Code) | 44 |
| Cache miss cost (Opus, 32K system) | ~$2.40/turn |
