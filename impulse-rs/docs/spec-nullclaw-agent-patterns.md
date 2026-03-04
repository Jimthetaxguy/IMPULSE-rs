# NullClaw Agent Patterns for Impulse

> **Date:** 2026-03-02
> **Status:** Draft
> **Type:** Impulse-only architectural spec
> **Source:** [`nullclaw-deep-analysis.md`](~/Documents/Research/AI_ML_Agents/Frameworks/nullclaw-deep-analysis.md)

---

## 1. Context

NullClaw is a Zig-based AI agent (678 KB binary, ~1 MB RAM, 3,230+ tests) that shares
significant architectural DNA with Impulse-rs: both use systems languages, SQLite as the
persistence layer, trait/vtable polymorphism, and target single-binary deployment.

The key difference is complementary, not competitive:

```
  Impulse MANAGES agents          NullClaw IS an agent
  ┌─────────────────────┐         ┌─────────────────────┐
  │  TUI multiplexer    │         │  Standalone binary   │
  │  ├─ Claude Code     │         │  ├─ 12+ providers    │
  │  ├─ Codex           │         │  ├─ 21 channels      │
  │  ├─ OpenCode        │         │  ├─ 9-stage RAG      │
  │  └─ (NullClaw?)     │◄────────│  └─ 7-layer sandbox  │
  │                     │         │                      │
  │  Workflow intel:     │         │  Raw capabilities:   │
  │  skills, hooks,     │         │  memory, security,   │
  │  plans, guardrails  │         │  messaging, hardware │
  └─────────────────────┘         └─────────────────────┘
```

This spec maps 5 NullClaw architectural patterns to Impulse's Rust codebase, evaluating
each for adoption priority.

---

## 2. Pattern 1: Vtable vs `dyn Trait`

### NullClaw's Approach

Every major subsystem uses `ptr: *anyopaque` + `vtable: *const VTable` — manual vtable
dispatch with zero heap allocation. Example from `src/security/sandbox.zig`:

```zig
pub const Sandbox = struct {
    ptr: *anyopaque,
    vtable: *const VTable,

    pub const VTable = struct {
        wrapCommand: *const fn (ctx: *anyopaque, argv: []const []const u8, ...) anyerror![]const []const u8,
        isAvailable: *const fn (ctx: *anyopaque) bool,
        name: *const fn (ctx: *anyopaque) []const u8,
        description: *const fn (ctx: *anyopaque) []const u8,
    };
};
```

### Impulse's Approach

Impulse uses Rust's `dyn Trait` with `Box` allocation. Key dispatch points:

- `src/llm_backends/mod.rs:58` — `Agent { provider: Box<dyn LlmProvider> }`
- `src/llm_backends/factory.rs:15` — `trait UnifiedAgent: Send + Sync`
- `src/llm_backends/factory.rs:118` — `AgentManager` creates `Box<dyn UnifiedAgent>`

### Trade-off Analysis

```
  ┌──────────────────┬───────────────────────┬──────────────────────┐
  │ Dimension        │ NullClaw (manual vtable)│ Impulse (dyn Trait)  │
  ├──────────────────┼───────────────────────┼──────────────────────┤
  │ Heap allocation  │ Zero (static vtable)  │ One Box per trait obj│
  │ Pointer size     │ 2 × usize (explicit)  │ 2 × usize (fat ptr) │
  │ Type safety      │ None (anyopaque)      │ Full compiler checks │
  │ Ergonomics       │ Manual boilerplate    │ derive, blanket impls│
  │ Performance      │ Identical dispatch    │ Identical dispatch   │
  │ Debug experience │ Opaque pointers       │ Full RTTI in debug   │
  │ Target hardware  │ $5 ARM, no allocator  │ Workstation, alloc OK│
  └──────────────────┴───────────────────────┴──────────────────────┘
```

### Recommendation: **Stay with `dyn Trait`**

Rust's fat pointer IS a vtable under the hood — the compiler generates the same dispatch
table NullClaw builds manually. The single `Box` allocation per trait object is negligible
on Impulse's workstation target (vs NullClaw's embedded target where even one allocation
matters). Rust's type safety and ergonomics are significant advantages.

**One optimization:** Where ownership isn't needed, prefer `&dyn Trait` over
`Box<dyn Trait>` to eliminate the heap allocation entirely. For example,
`search_history()` in `src/retrieval/query.rs` could accept `&dyn EmbeddingProvider`
instead of owning the provider.

---

## 3. Pattern 2: BackendDescriptor / Capability Registry

### NullClaw's Approach

Each memory backend declares its capabilities at compile time via `BackendDescriptor`:

```zig
pub const BackendDescriptor = struct {
    name: []const u8,
    label: []const u8,
    auto_save_default: bool,
    capabilities: BackendCapabilities,  // bitflags
    needs_db_path: bool,
    needs_workspace: bool,
    create: *const fn (Allocator, BackendConfig) anyerror!BackendInstance,
};
```

The `BackendCapabilities` bitflags encode what each backend supports: `keyword_rank`,
`transactions`, `vector_search`, `hybrid_merge`, etc. The registry can query "which
backends support vector search?" without instantiating them.

### Impulse's Current State

`SearchBackend` at `src/retrieval/types.rs:28` is a flat enum:

```rust
pub enum SearchBackend {
    Auto,        // capabilities: implicit (all)
    SqliteVec,   // capabilities: implicit (vector + keyword)
    RustCosine,  // capabilities: implicit (vector only)
    Keyword,     // capabilities: implicit (keyword only)
}
```

Capabilities are implicit — encoded in match arms scattered across `query.rs` rather
than declared on the enum. `resolve_semantic_backends()` in `query.rs` hardcodes the
priority chain `[SqliteVec, RustCosine]` instead of querying a capability registry.

### Proposed: BackendDescriptor for Impulse

```rust
use bitflags::bitflags;

bitflags! {
    pub struct BackendCapabilities: u32 {
        const KEYWORD_SEARCH  = 0b0000_0001;
        const VECTOR_SEARCH   = 0b0000_0010;
        const HYBRID_MERGE    = 0b0000_0100;
        const TRANSACTIONS    = 0b0000_1000;
        const PERSISTENCE     = 0b0001_0000;
        const KNN_NATIVE      = 0b0010_0000;
    }
}

pub struct BackendDescriptor {
    pub name: &'static str,
    pub label: &'static str,
    pub capabilities: BackendCapabilities,
    pub priority: u8,  // lower = preferred
}

impl SearchBackend {
    pub fn descriptor(&self) -> BackendDescriptor {
        match self {
            Self::SqliteVec => BackendDescriptor {
                name: "sqlite-vec",
                label: "SQLite vec0 Extension",
                capabilities: BackendCapabilities::KEYWORD_SEARCH
                    | BackendCapabilities::VECTOR_SEARCH
                    | BackendCapabilities::HYBRID_MERGE
                    | BackendCapabilities::KNN_NATIVE
                    | BackendCapabilities::PERSISTENCE,
                priority: 1,
            },
            Self::RustCosine => BackendDescriptor {
                name: "rust-cosine",
                label: "Rust In-Memory Cosine",
                capabilities: BackendCapabilities::VECTOR_SEARCH,
                priority: 2,
            },
            // ...
        }
    }
}
```

### Recommendation: **Adopt for retrieval backends**

This enables the 9-stage pipeline (Spec 1) to query "which backends support vector
search?" programmatically instead of hardcoding backend chains. Priority: **Medium** —
implement when building the pipeline, not before.

---

## 4. Pattern 3: MCP-as-Vtable (Tools Unification)

### NullClaw's Approach

From `src/mcp.zig`: MCP tools discovered via the initialize handshake are wrapped into
the same `Tool` vtable as native tools. The agent cannot distinguish between built-in
file-read and an MCP-provided file-read — perfect abstraction.

```
  ┌─────────────────────────────────────┐
  │           Agent Core                │
  │                                     │
  │   tool.call("read_file", args)      │
  │        │                            │
  │        ▼                            │
  │   ┌─────────┐    ┌─────────┐       │
  │   │ Native  │    │  MCP    │       │
  │   │ VTable  │    │ VTable  │       │
  │   └────┬────┘    └────┬────┘       │
  │        │              │             │
  │   fs::read()    json-rpc → server  │
  └─────────────────────────────────────┘
```

### Impulse's Current Position

Impulse is an MCP **server** (exposes tools TO agents via `impulse-mcp-server`), not an
MCP **client** (consumes tools FROM external servers). The `UnifiedAgent` trait at
`factory.rs:15` dispatches to CLI or API backends, but doesn't consume MCP tools.

### Recommendation: **Defer**

This pattern requires Impulse to add MCP client capability — consuming tools from
external MCP servers and routing them through the agent pipeline. This is a significant
architectural addition that should wait until:

1. MCP client support is added to Impulse's roadmap
2. Use cases emerge (e.g., consuming NullClaw's tools via MCP)
3. The CLI provider abstraction (Spec 3) is stable

Priority: **Low** — track but don't implement yet.

---

## 5. Pattern 4: NullClaw as Impulse Panel

### Complementary Architecture

NullClaw provides raw capabilities (21 channels, hardware, RAG, sandbox) that Impulse's
workflow intelligence (skill chains, hookify, session lifecycle) can govern. Together
they form a complete stack: NullClaw as a capable agent panel inside Impulse's TUI,
alongside Claude Code and Codex.

### Integration Path

NullClaw integration follows from Spec 3 (CLI Provider). The steps:

1. Add `AgentType::NullClaw` variant to `src/llm_backends/types.rs:27`
2. Implement `CliProtocol` for NullClaw's CLI interface
3. NullClaw exposes `nullclaw --prompt "..." --output json` mode
4. Impulse spawns NullClaw as a panel, routes tasks via `CliAgent`

### Recommendation: **Document the path, wait for stability**

NullClaw is 2 weeks old (created 2026-02-16). Its CLI interface and output formats will
change. Document the integration architecture now; implement after NullClaw reaches a
stable release (v0.5+ or 3+ months of API stability).

Priority: **Low** — architectural documentation only for now.

---

## 6. Pattern 5: Circuit Breaker + Outbox

### NullClaw's Approach

NullClaw uses two resilience patterns for its vector storage layer:

**Circuit Breaker** — When a vector store fails N times, the breaker opens and all
requests short-circuit to fallback for a cooldown period:

```
  CLOSED ──(failure)──▶ HALF-OPEN ──(success)──▶ CLOSED
     ▲                      │
     │                  (failure)
     │                      │
     │                      ▼
     └────(cooldown)──── OPEN ──▶ returns fallback
```

**Outbox** — Durable async sync: writes go to a local outbox table first, then a
background process syncs to the vector store. If the store is down, writes queue in
the outbox until it recovers.

### Impulse's Current State

`src/retrieval/embedding.rs` has timeout handling (10ms polling loop with deadline
check, explicit `child.kill()` on timeout) but no circuit breaker. If the embedding
subprocess fails, each subsequent call retries from scratch — no failure memory.

The `FallbackCode` enum at `src/retrieval/types.rs:99` tracks failure reasons:

```rust
pub enum FallbackCode {
    VectorBackendDisabled,
    SqliteVecUnavailable,
    EmbeddingTimeout,
    EmbeddingSpawnFailed,
    EmbeddingProcessFailed,
    EmbeddingNoVector,
    EmbeddingDimensionMismatch,
    RetrievalDbError,
    RetrievalDbCorrupt,
    IndexLockActive,
}
```

These are per-request failure codes — no state is maintained across requests. If the
embedding subprocess times out 10 times in a row, the 11th call still spawns a new
process and waits for timeout.

### Proposed: CircuitBreaker for Embedding Pipeline

```rust
pub struct CircuitBreaker {
    failure_count: AtomicU32,
    threshold: u32,
    last_failure: AtomicU64,
    half_open_after: Duration,
    state: AtomicU8,  // Closed=0, Open=1, HalfOpen=2
}
```

Integration: wrap `embed_texts()` calls in `query.rs`. When the breaker opens,
semantic search immediately falls back to keyword instead of waiting for timeout.

### Recommendation: **Adopt circuit breaker, defer outbox**

The circuit breaker prevents cascade failures in the embedding pipeline — a clear win
with minimal complexity. The outbox pattern is overkill until Impulse has a remote
vector store (currently everything is local SQLite).

Priority: **Medium** (circuit breaker), **Low** (outbox).

---

## 7. Adoption Matrix

```
  ┌─────────────────────────────┬──────────┬────────┬──────────────────────────┐
  │ Pattern                     │ Priority │ Adopt? │ Where in Impulse         │
  ├─────────────────────────────┼──────────┼────────┼──────────────────────────┤
  │ 1. Vtable vs dyn Trait      │ —        │ No     │ Already using dyn Trait  │
  │    (optimize: &dyn where    │ Low      │ Yes    │ query.rs, factory.rs     │
  │     ownership unneeded)     │          │        │                          │
  │                             │          │        │                          │
  │ 2. BackendDescriptor /      │ Medium   │ Yes    │ retrieval/types.rs       │
  │    Capability Registry      │          │        │ (alongside pipeline)     │
  │                             │          │        │                          │
  │ 3. MCP-as-Vtable            │ Low      │ Defer  │ Future MCP client        │
  │                             │          │        │                          │
  │ 4. NullClaw as Panel        │ Low      │ Defer  │ llm_backends/types.rs    │
  │                             │          │        │ (after NullClaw stable)  │
  │                             │          │        │                          │
  │ 5. Circuit Breaker          │ Medium   │ Yes    │ retrieval/query.rs       │
  │    (Outbox: defer)          │          │        │ (wrap embed_texts calls) │
  └─────────────────────────────┴──────────┴────────┴──────────────────────────┘
```

**Summary:** Adopt 2 patterns (BackendDescriptor, Circuit Breaker), optimize 1 existing
pattern (`&dyn` where possible), defer 2 patterns (MCP-as-Vtable, NullClaw panel).
The key insight is that Rust's `dyn Trait` already provides what NullClaw achieves with
manual vtables — the real gaps are in capability metadata and failure resilience.

---

## 8. Cross-References

- **NullClaw Analysis:** [`nullclaw-deep-analysis.md`](~/Documents/Research/AI_ML_Agents/Frameworks/nullclaw-deep-analysis.md)
- **Spec 1 — Retrieval Pipeline:** [`spec-retrieval-pipeline-upgrade.md`](./spec-retrieval-pipeline-upgrade.md) — uses BackendDescriptor for stage routing
- **Spec 2 — Sandbox Integration:** [`spec-sandbox-integration.md`](./spec-sandbox-integration.md) — vtable comparison informs Sandbox trait design
- **Spec 3 — CLI Provider:** [`spec-cli-provider-extension.md`](./spec-cli-provider-extension.md) — CliProtocol enables NullClaw-as-panel
- **Impulse source:**
  - `src/llm_backends/mod.rs:50` — `LlmProvider` trait
  - `src/llm_backends/mod.rs:58` — `Agent` struct with `Box<dyn LlmProvider>`
  - `src/llm_backends/factory.rs:15` — `UnifiedAgent` trait
  - `src/llm_backends/factory.rs:118` — `AgentManager`
  - `src/llm_backends/cli.rs:65` — `CliAgent` struct
  - `src/llm_backends/cli.rs:239` — `send_message()` stub
  - `src/llm_backends/types.rs:27` — `AgentType` enum
  - `src/retrieval/types.rs:28` — `SearchBackend` enum
  - `src/retrieval/types.rs:99` — `FallbackCode` enum
  - `src/retrieval/query.rs:16` — `cosine_similarity()`
  - `src/retrieval/embedding.rs` — embedding subprocess with timeout
  - `src/guardrail/types.rs:10` — `GuardAction` enum
- **NullClaw source (for reference):**
  - `src/security/sandbox.zig` — Sandbox vtable
  - `src/memory/engines/registry.zig` — BackendDescriptor
  - `src/memory/retrieval/engine.zig` — RetrievalSourceAdapter
  - `src/providers/claude_cli.zig` — CLI-as-provider
  - `src/mcp.zig` — MCP tool wrapping
