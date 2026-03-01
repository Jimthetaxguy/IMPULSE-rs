# Rust + WASM Enterprise AI Agent Platform — Architecture Plan

> **Document Version:** 1.0  
> **Date:** March 1, 2026  
> **Classification:** Internal Technical Reference  
> **Audience:** Engineering leads, platform architects, senior engineers  
> **Source Research:** Files 01–10 in `/research/`, compiled from GitHub, crates.io, and primary sources

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [License Audit Summary](#2-license-audit-summary)
3. [Recommended Technology Stack (by Layer)](#3-recommended-technology-stack-by-layer)
4. [Architecture Diagram (ASCII)](#4-architecture-diagram-ascii)
5. [WASM Agent Sandboxing Deep Dive](#5-wasm-agent-sandboxing-deep-dive)
6. [Cargo Workspace Structure](#6-cargo-workspace-structure)
7. [Critical Deprecation Warnings](#7-critical-deprecation-warnings)
8. [Performance Targets](#8-performance-targets)
9. [Build & Deploy Pipeline](#9-build--deploy-pipeline)
10. [Risk Assessment & Gaps](#10-risk-assessment--gaps)
11. [Phased Implementation Roadmap](#11-phased-implementation-roadmap)

---

## 1. Executive Summary

### Platform Vision

We are building an **enterprise-grade, multi-tenant AI agent platform** targeting the tax diligence domain, written primarily in Rust with WebAssembly sandboxing for agent isolation. The core thesis: Rust's memory safety and performance characteristics, combined with WASM's capability-based security model, produce a platform that is simultaneously more secure, more performant, and more cost-efficient than equivalent Python or Node.js stacks.

### Why Rust + WASM

**Performance evidence from benchmarks (Feb 2026):** Rust agent frameworks use 5× less memory than Python equivalents (1.0–1.1 GB vs 4.7–5.7 GB peak), achieve 15–34× faster cold starts (4 ms vs 54–138 ms), and deliver 36% higher throughput. The bottleneck for most AI workloads is the LLM API round-trip — not the framework — but memory efficiency directly impacts hosting cost at scale.

**Security evidence from production deployments:** American Express (KubeCon 2024) runs a multi-tenant FaaS platform on wasmCloud where each business function is a WASM component with strictly declared capabilities. Cosmonic demonstrated at KubeCon EU 2025 that even with full remote code execution inside a WASM sandbox, an attacker cannot pivot — no shell, no filesystem, no network beyond declared WIT interface imports.

**Production validation:** Cloudflare Workers serves trillions of requests, Fastly Compute runs 100,000+ WASM isolates per CPU core, Shopify Functions execute <5ms at Black Friday scale, Google Sheets ships WasmGC in production with 2× faster calculations.

### License-First Approach

Every dependency must carry MIT, Apache-2.0, or dual MIT/Apache-2.0 licensing. This document flags all exceptions explicitly. The Rust ecosystem is almost entirely MIT/Apache — we found no GPL, LGPL, or proprietary licenses in the primary stack. Notable exceptions requiring legal review: `webauthn-rs` (MPL-2.0, file-scoped copyleft), `mdBook` (MPL-2.0, docs use only), Extism (BSD-3-Clause, acceptable), and the VectorChord successor to pgvecto.rs (AGPL/ELv2 — **avoid without commercial license**).

### Key Architectural Decisions Summary

| Decision | Choice | Rationale |
|---|---|---|
| Agent framework | Rig (v0.31.0, MIT, 4,600+ stars) | Most production-ready Rust agent framework |
| WASM runtime | Wasmtime (Apache-2.0, 17,500 stars) | Only runtime with full Component Model support |
| Web API | Axum 0.8 + Tower + Tonic | Tokio-team backed, Tower ecosystem, 248M+ downloads |
| Frontend | Leptos 0.8 (SSR) or hybrid with TS | Best SSR story in Rust WASM ecosystem |
| Database | SQLx 0.8 + Qdrant | Compile-time SQL safety + leading vector store |
| Auth | jsonwebtoken + casbin + rustls | Industry standard JWT, domain RBAC, audited TLS |
| Observability | tracing + OpenTelemetry + Langfuse OTLP | Instrument once, export anywhere |
| MCP | rmcp 0.16.0 (official SDK, MIT) | Official Anthropic org, all transports supported |

---

## 2. License Audit Summary

The following master table covers every recommended crate and tool. **GREEN** = MIT or Apache-2.0, fully permissive, no legal review needed. **YELLOW** = Permissive but non-standard license, run by legal before production use. **RED** = Copyleft or proprietary, requires legal review and explicit approval.

### 2.1 Core Runtime & Async

| Crate | Version | License | Status | Notes |
|---|---|---|---|---|
| `tokio` | 1.49.0 | MIT | 🟢 GREEN | 542M+ downloads, industry standard |
| `tokio-stream` | 0.1.x | MIT | 🟢 GREEN | Part of tokio monorepo |
| `tokio-util` | 0.7.x | MIT | 🟢 GREEN | CancellationToken, Codec |
| `futures` | 0.3.32 | MIT OR Apache-2.0 | 🟢 GREEN | Runtime-agnostic, WASM-compatible |
| `futures-core` | 0.3.31 | MIT OR Apache-2.0 | 🟢 GREEN | Stream trait foundation |
| `async-trait` | 0.1.x | MIT OR Apache-2.0 | 🟢 GREEN | dyn Trait async support |
| `async-channel` | 2.x | MIT OR Apache-2.0 | 🟢 GREEN | WASM-compatible MPMC |
| `flume` | 0.11.x | MIT OR Apache-2.0 | 🟢 GREEN | Sync+async bridge channel |
| `async-stream` | 0.3.6 | MIT | 🟢 GREEN | stream! macro for SSE |

### 2.2 Agent Framework & LLM Clients

| Crate | Version | License | Status | Notes |
|---|---|---|---|---|
| `rig-core` | 0.31.0 | MIT | 🟢 GREEN | Primary agent framework, 4,600+ stars |
| `adk-rust` | 0.3.1 | Apache-2.0 | 🟢 GREEN | Enterprise features (RBAC, voice) |
| `async-openai` | 0.27.x | MIT | 🟢 GREEN | Production OpenAI client, 1,600+ stars |
| `genai` | 0.5.x | MIT | 🟢 GREEN | Best multi-provider coverage, 668 stars |
| `ollama-rs` | 0.3.4 | MIT | 🟢 GREEN | Local Ollama client, 922 stars |
| `mistralrs` | 0.7.0 | MIT | 🟢 GREEN | Local inference, 6,100 stars |

### 2.3 WASM Runtime & Component Model

| Crate/Tool | Version | License | Status | Notes |
|---|---|---|---|---|
| `wasmtime` | 27.x | Apache-2.0 | 🟢 GREEN | 17,500 stars, full Component Model |
| `wit-bindgen` | 0.46.0 | Apache-2.0 OR MIT | 🟢 GREEN | WIT code generation |
| `wasm-tools` | 1.x CLI | Apache-2.0 OR MIT | 🟢 GREEN | Validation, inspection |
| `cargo-component` | 0.21.1 | Apache-2.0 | 🟢 GREEN | WASM component build tool (experimental) |
| Extism | 1.12.0 | BSD-3-Clause | 🟡 YELLOW | BSD-3 permissive but not MIT/Apache |
| wasmCloud | 1.9.0 | Apache-2.0 | 🟢 GREEN | CNCF Incubating, AI agent sandboxing |

### 2.4 Web API & Middleware

| Crate | Version | License | Status | Notes |
|---|---|---|---|---|
| `axum` | 0.8.8 | MIT | 🟢 GREEN | 25,100 stars, 248M+ downloads |
| `axum-extra` | 0.10.x | MIT | 🟢 GREEN | TypedPath, JsonLines, Protobuf |
| `tower` | 0.5.1 | MIT | 🟢 GREEN | 6,209 dependent crates |
| `tower-http` | 0.6.8 | MIT | 🟢 GREEN | CORS, auth, tracing, compression |
| `hyper` | 1.8.1 | MIT | 🟢 GREEN | Foundation of axum/reqwest |
| `reqwest` | 0.13.x | MIT OR Apache-2.0 | 🟢 GREEN | 380M+ downloads |
| `tonic` | 0.14.5 | MIT | 🟢 GREEN | gRPC standard, 11,400 stars |
| `prost` | 0.14.3 | Apache-2.0 | 🟢 GREEN | Protobuf, 25.5M/month downloads |
| `utoipa` | 5.x | MIT OR Apache-2.0 | 🟢 GREEN | OpenAPI 3.1 generation |
| `tokio-tungstenite` | 0.26.x | MIT | 🟢 GREEN | WebSocket client |

### 2.5 Frontend (WASM)

| Crate/Tool | Version | License | Status | Notes |
|---|---|---|---|---|
| `leptos` | 0.8.11 | MIT | 🟢 GREEN | 19,300 stars, best SSR in Rust |
| `dioxus` | 0.6.3 | MIT OR Apache-2.0 | 🟢 GREEN | 31,200 stars, cross-platform |
| `wasm-bindgen` | 0.2.100 | MIT OR Apache-2.0 | 🟢 GREEN | Foundation of all Rust WASM |
| `web-sys` | 0.3.77 | MIT OR Apache-2.0 | 🟢 GREEN | Web API bindings |
| `gloo` | 0.10.0 | MIT OR Apache-2.0 | 🟢 GREEN | Ergonomic browser API wrappers |
| `cargo-leptos` | 0.2.45 | MIT | 🟢 GREEN | Leptos full-stack build tool |
| Trunk | 0.21.14 | MIT OR Apache-2.0 | 🟢 GREEN | WASM bundler / dev server |
| wasm-pack | 0.13.1 | MIT OR Apache-2.0 | 🟢 GREEN | npm-compatible WASM packaging |

### 2.6 Database & Vector Store

| Crate | Version | License | Status | Notes |
|---|---|---|---|---|
| `sqlx` | 0.8.x | MIT OR Apache-2.0 | 🟢 GREEN | 15,900 stars, compile-time SQL checks |
| `sea-orm` | 1.x | MIT OR Apache-2.0 | 🟢 GREEN | ORM built on SQLx, v1.0 stable |
| `qdrant-client` | 1.17.0 | Apache-2.0 | 🟢 GREEN | Official Qdrant Rust client |
| `pgvector` | 0.4.x | MIT | 🟢 GREEN | Postgres vector extension client |
| `lancedb` | 0.14.x | Apache-2.0 | 🟢 GREEN | Embedded serverless vector DB |
| `bb8` | 0.9.1 | MIT | 🟢 GREEN | Tokio async connection pool |
| `deadpool` | 0.12.x | MIT OR Apache-2.0 | 🟢 GREEN | Multi-backend async pool |
| `tokio-postgres` | 0.7.x | MIT OR Apache-2.0 | 🟢 GREEN | Pure Rust async Postgres driver |
| pgvecto.rs | 0.4.0 | Apache-2.0 | 🔴 RED | Successor VectorChord uses AGPL/ELv2! |

### 2.7 Caching & Message Queue

| Crate | Version | License | Status | Notes |
|---|---|---|---|---|
| `moka` | 0.12.13 | MIT OR Apache-2.0 | 🟢 GREEN | 2,200 stars, powers crates.io |
| `redis` | 1.0.x | MIT OR Apache-2.0 | 🟢 GREEN | Verify exact version license |
| `cached` | 0.54.x | MIT | 🟢 GREEN | Function memoization |
| `async-nats` | 0.46.0 | Apache-2.0 | 🟢 GREEN | Official NATS client, 1,300 stars |
| `rdkafka` | 0.25.x | MIT | 🟢 GREEN | Kafka (librdkafka-based), 1,900 stars |
| `lapin` | 4.1.0 | MIT | 🟢 GREEN | RabbitMQ/AMQP, 1,200 stars |

### 2.8 Auth & Security

| Crate | Version | License | Status | Notes |
|---|---|---|---|---|
| `jsonwebtoken` | 10.3.0 | MIT | 🟢 GREEN | 100M+ downloads, 6,500 stars |
| `oauth2` | 5.0.0 | MIT OR Apache-2.0 | 🟢 GREEN | 26M+ downloads |
| `openidconnect` | 4.0.1 | MIT | 🟢 GREEN | OIDC Core 1.0 compliant |
| `axum-login` | 0.18.0 | MIT | 🟢 GREEN | Session-based auth for Axum |
| `tower-sessions` | 0.15.0 | MIT | 🟢 GREEN | Pluggable session store |
| `argon2` | 0.5.x | MIT OR Apache-2.0 | 🟢 GREEN | PHC winner, use 0.5.x stable |
| `casbin` | 2.20.0 | Apache-2.0 | 🟢 GREEN | RBAC/ABAC with domain support |
| `rustls` | 0.23.37 | Apache-2.0 OR ISC OR MIT | 🟢 GREEN | 520M+ downloads, zero-unsafe core |
| `aes-gcm` | 0.10.x | Apache-2.0 OR MIT | 🟢 GREEN | RustCrypto AES-GCM |
| `sha2` | 0.10.x | MIT OR Apache-2.0 | 🟢 GREEN | 490M+ downloads |
| `p256` | 0.13.x | Apache-2.0 OR MIT | 🟢 GREEN | NIST P-256 ECDH/ECDSA |
| `orion` | 0.17.13 | MIT | 🟢 GREEN | sodiumoxide replacement, pure Rust |
| `rcgen` | 0.14.7 | MIT OR Apache-2.0 | 🟢 GREEN | X.509 certificate generation |
| `governor` | 0.10.4 | MIT | 🟢 GREEN | GCRA rate limiting, WASM-compatible |
| `tower-governor` | 0.8.0 | MIT OR Apache-2.0 | 🟢 GREEN | Governor as Tower middleware |
| `vaultrs` | 0.7.4 | MIT | 🟢 GREEN | Async Vault client |
| `webauthn-rs` | 0.5.4 | **MPL-2.0** | 🟡 YELLOW | File-scoped copyleft; use `passkey` instead |
| `passkey` | 0.5.0 | MIT OR Apache-2.0 | 🟢 GREEN | 1Password implementation, alternative |
| `ring` | 0.17.14 | Apache-2.0 AND ISC | 🟢 GREEN | 422M+ downloads; hard to cross-compile |
| `sodiumoxide` | 0.2.7 | MIT OR Apache-2.0 | 🔴 RED | **DEPRECATED AND ARCHIVED Sept 2022** |

### 2.9 Observability & Tracing

| Crate | Version | License | Status | Notes |
|---|---|---|---|---|
| `tracing` | 0.1.41 | MIT | 🟢 GREEN | 6,300 stars, 437K dependent crates |
| `tracing-subscriber` | 0.3.19 | MIT | 🟢 GREEN | Layer-composable subscriber |
| `tracing-appender` | 0.2.x | MIT | 🟢 GREEN | Non-blocking file appender |
| `tracing-opentelemetry` | 0.28.0 | MIT | 🟢 GREEN | tracing → OTel bridge |
| `opentelemetry` | 0.27.x | Apache-2.0 | 🟢 GREEN | OTel API, 2,300 stars |
| `opentelemetry-sdk` | 0.27.x | Apache-2.0 | 🟢 GREEN | OTel SDK implementation |
| `opentelemetry-otlp` | 0.27.x | Apache-2.0 | 🟢 GREEN | OTLP export (Jaeger, Langfuse, etc.) |
| `metrics` | 0.23.x | MIT | 🟢 GREEN | Metrics facade, 1,400 stars |
| `metrics-exporter-prometheus` | 0.15.x | MIT | 🟢 GREEN | Prometheus metrics endpoint |
| `tiktoken-rs` | 0.6.x | MIT | 🟢 GREEN | OpenAI tokenization, 342 stars |
| `criterion` | 0.5/0.8 | MIT OR Apache-2.0 | 🟢 GREEN | Statistical benchmarking, 5,400 stars |
| `pprof` | 0.15.x | Apache-2.0 | 🟢 GREEN | CPU profiling, 1,500 stars |
| `opentelemetry-jaeger` | 0.22.0 | Apache-2.0 | 🔴 RED | **DEPRECATED** — use opentelemetry-otlp |
| `opentelemetry-prometheus` | 0.29.x | Apache-2.0 | 🔴 RED | **DISCONTINUED** — security vuln |

### 2.10 MCP & Serialization

| Crate | Version | License | Status | Notes |
|---|---|---|---|---|
| `rmcp` | 0.16.0 | MIT | 🟢 GREEN | Official MCP SDK, 2,500 stars |
| `serde` | 1.0.228 | MIT OR Apache-2.0 | 🟢 GREEN | 43M/month downloads, 10,100 stars |
| `serde_json` | 1.0.149 | MIT OR Apache-2.0 | 🟢 GREEN | 44.9M/month downloads |
| `toml` | 1.0.x | MIT OR Apache-2.0 | 🟢 GREEN | 31.2M/month; preferred config format |
| `postcard` | 1.1.3 | MIT OR Apache-2.0 | 🟢 GREEN | no_std binary; bincode replacement |
| `schemars` | 1.2.1 | MIT | 🟢 GREEN | JSON Schema generation, 23.6M/month |
| `garde` | 0.22.1 | MIT OR Apache-2.0 | 🟢 GREEN | Context-aware validation |
| `config` | 0.15.19 | MIT OR Apache-2.0 | 🟢 GREEN | Layered configuration |
| `dotenvy` | 0.15.7 | MIT | 🟢 GREEN | .env loading (replaces dotenv) |
| `bincode` | 3.0.0 | MIT | 🔴 RED | **UNMAINTAINED** RUSTSEC-2025-0141 |
| `serde_yaml` | 0.9.34 | MIT OR Apache-2.0 | 🟡 YELLOW | **DEPRECATED** by author — use TOML |
| `dotenv` | — | MIT | 🔴 RED | **UNMAINTAINED** RUSTSEC-2021-0141 |

### 2.11 Testing & CI/CD

| Crate/Tool | Version | License | Status | Notes |
|---|---|---|---|---|
| `cargo-nextest` | 0.9.106 | MIT OR Apache-2.0 | 🟢 GREEN | 3–5× faster test runner |
| `mockall` | 0.14.0 | MIT AND Apache-2.0 | 🟢 GREEN | Mock generation, 1,700 stars |
| `wiremock-rs` | 0.6.x | MIT OR Apache-2.0 | 🟢 GREEN | HTTP mocking, 738 stars |
| `rstest` | 0.26.1 | MIT OR Apache-2.0 | 🟢 GREEN | Fixture + parameterized tests |
| `proptest` | 1.10.0 | MIT OR Apache-2.0 | 🟢 GREEN | Property-based testing, 1,900 stars |
| `insta` | 1.46.3 | Apache-2.0 | 🟢 GREEN | Snapshot testing, 2,600 stars |
| `testcontainers-rs` | 0.25.0 | MIT OR Apache-2.0 | 🟢 GREEN | Docker integration tests |
| `cargo-deny` | 0.18.5 | MIT OR Apache-2.0 | 🟢 GREEN | License + security auditing |
| `cargo-audit` | 0.21.2 | MIT OR Apache-2.0 | 🟢 GREEN | CVE auditing |
| `cargo-llvm-cov` | 0.6.19 | MIT OR Apache-2.0 | 🟢 GREEN | Cross-platform code coverage |
| `sccache` | 0.12.0 | Apache-2.0 | 🟢 GREEN | Compilation caching, 6,700 stars |
| `cargo-chef` | 0.1.72 | MIT OR Apache-2.0 | 🟢 GREEN | Docker layer caching, 2,300 stars |
| mold | 2.40.4 | MIT | 🟢 GREEN | Fast linker, 15,800 stars |
| `cargo-release` | 1.1.1 | MIT OR Apache-2.0 | 🟢 GREEN | Automated release pipeline |
| wasm-opt (Binaryen) | version_124 | Apache-2.0 | 🟢 GREEN | WASM optimizer, 8,100 stars |
| `twiggy` | 0.7.0 | MIT OR Apache-2.0 | 🟢 GREEN | WASM size profiler |
| mdBook | 0.5.2 | **MPL-2.0** | 🟡 YELLOW | Docs use OK; don't modify binary |
| `iai-callgrind` | 0.17.2 | MIT OR Apache-2.0 | 🟢 GREEN | Deterministic CI benchmarks |

---

## 3. Recommended Technology Stack (by Layer)

### 3.1 Async Runtime — Tokio Ecosystem

**PRIMARY PICK:** `tokio` v1.49.0 (MIT) with full feature set

**Justification:**
- 542,872,333 all-time downloads; used by 541,000+ GitHub repositories
- Industry standard: Amazon, Discord, Cloudflare, Dropbox
- Multi-threaded work-stealing scheduler; bare-metal performance with millions of tasks/second
- LTS policy: 1.43.x (until Mar 2026), 1.47.x (until Sep 2026); enterprise-grade stability guarantee
- Every other framework in this stack (Axum, Tonic, SQLx, Rig) requires Tokio

**Full runtime stack:**
```toml
tokio = { version = "1", features = ["full"] }
tokio-stream = "0.1"
tokio-util = { version = "0.7", features = ["codec"] }
futures = "0.3"
async-trait = "0.1"
async-stream = "0.3"
flume = "0.11"          # sync+async bridge; WASM-compatible
async-channel = "2"     # runtime-agnostic MPMC; WASM-compatible
```

**Alternatives considered:**
- `async-std` — smaller ecosystem, fewer production deployments; dropped in 2024 by many projects
- `smol` — elegant but minimal ecosystem; not suitable for enterprise AI workloads requiring full HTTP/gRPC stack
- Single-threaded runtimes for WASM — applicable for WASM plugin execution, not the host runtime

**Known limitations:** Tokio does not compile to standard `wasm32-unknown-unknown`. For WASM guest code (agent plugins), use `futures-core`, `async-channel`, and `flume` which are WASM-compatible. The host runtime always runs native Tokio.

---

### 3.2 Agent Framework — Rig

**PRIMARY PICK:** `rig-core` v0.31.0 (MIT, 4,600+ GitHub stars, 106 contributors)

**Justification:**
- Most production-ready Rust agent framework as of March 2026
- Known production deployments: Nethermind (NINE engine), Linera Protocol, Dria Compute Node, The MCP Rust SDK
- 20+ model providers under a single unified interface
- 10+ vector store integrations for RAG
- Full MCP (Model Context Protocol) support
- WASM-compatible for `rig-core` (unique advantage — other frameworks lack this)
- GenAI Semantic Convention (OpenTelemetry) compatibility built in
- Streaming, tool calling, multi-turn conversations, embedding workflows

**Benchmark performance (Feb 2026, [AutoAgents study](https://dev.to/saivishwak/benchmarking-ai-agent-frameworks-in-2026-autoagents-rust-vs-langchain-langgraph-llamaindex-338f)):**
- Avg latency: 6,065 ms (P95: 10,131 ms) — primarily LLM API bound
- Throughput: 4.44 rps
- Peak memory: ~1,019 MB vs Python frameworks at 4.7–5.7 GB
- Cold start: **4 ms** vs 54–138 ms for Python frameworks (15–34× faster)

**Alternatives considered:**

| Framework | Stars | Reason Not Primary |
|---|---|---|
| ADK-Rust (v0.3.1) | 148 | Too new; impressive features (RBAC, voice, 120+ examples) but limited production track record; use as secondary framework for advanced enterprise features |
| AutoAgents | 92 | Top benchmark performer (4.97 rps), built-in WASM for tools, but small community and limited ecosystem |
| swarms-rs | 91 | Enterprise claims but no production evidence; Sep 2025 last release |
| langchain-rust | 1,100 | Last release Oct 2024; community shifted to Rig |

**Recommended hybrid:** Use Rig as the primary framework. Evaluate ADK-Rust v0.3.1 for features not in Rig: RBAC, realtime voice agents, SSO/OAuth, audit logging. These are all Apache-2.0 licensed and can coexist.

**Integration pattern:**
```toml
rig-core = { version = "0.31", features = ["derive"] }
adk-rust = { version = "0.3", features = ["openai", "anthropic"] }  # optional
```

---

### 3.3 LLM Client — genai + async-openai

**PRIMARY PICK (multi-provider):** `genai` v0.5.x (MIT, 668 stars) for multi-provider workflows  
**PRIMARY PICK (OpenAI-specific):** `async-openai` v0.27.x (MIT, 1,600+ stars) for OpenAI/Azure/OpenRouter

**Justification for genai:**
- 14+ providers with **native protocol implementations** (not just forwarding)
- Supports DeepSeek R1 `reasoning_content`, Gemini "Thinking", Anthropic "Reasoning" natively
- PDF, image, and embedding support unified across providers
- Normalized chat API across all providers
- Maintained by Jeremy Chone, known for high-quality Rust content

**Justification for async-openai:**
- De facto standard Rust OpenAI client (used by Rig, langchain-rust, and most AI projects)
- Tracks official OpenAI API spec exactly; exponential backoff retry
- WASM companion crate (`async-openai-wasm`) available
- Feb 2026 release; most actively maintained OpenAI client

**Provider matrix:**

| Provider | Best Client |
|---|---|
| OpenAI / Azure | `async-openai` |
| Anthropic | `genai` |
| Google Gemini | `genai` |
| DeepSeek (with reasoning) | `genai` |
| Groq / Fireworks / Together | `genai` |
| Local Ollama | `ollama-rs` v0.3.4 (MIT, 922 stars) |
| Local inference (CUDA/Metal) | `mistralrs` v0.7.0 (MIT, 6,100 stars) |

**Alternative that was considered:** `llm` (graniet, 239 stars) — most feature-rich with voice/TTS/agents, but lower adoption than `genai`. Use as supplementary if needed.

**⚠️ Name collision warning:** The `llm` crate on crates.io now refers to `graniet/llm` (multi-provider API library), NOT the archived `rustformers/llm` local inference engine (archived June 2024). Always specify the full path when referring to either.

```toml
genai = "0.5"
async-openai = "0.27"
ollama-rs = "0.3"
mistralrs = { version = "0.7", optional = true }  # heavy dep; make optional
```

---

### 3.4 WASM Runtime & Sandboxing — Wasmtime + Component Model

**PRIMARY PICK:** `wasmtime` v27.x (Apache-2.0, 17,500+ GitHub stars)

**Justification:**
- **Only runtime with full Component Model support** as of early 2026
- Production-ready since v1.0 (September 2022)
- Key production deployments: Shopify Functions (millions of checkouts), Fastly Compute (100,000+ isolates/CPU core), DFINITY (150,000+ contracts), Microsoft Azure KService node pools
- WASI 0.2 (Preview 2) fully supported; WASI 0.3 experimental in latest releases
- 24/7 fuzzing via OSS Fuzz; formal Spectre mitigations
- Cranelift JIT + AOT compilation; 5-microsecond instantiation time (400× faster than earlier versions)
- Async calling context prevents blocking Tokio executor

**Key capabilities for tax diligence agents:**
```rust
use wasmtime::{Engine, Store, Config, StoreLimitsBuilder};

// Per-agent resource limits
let limits = StoreLimitsBuilder::new()
    .memory_size(64 * 1024 * 1024)  // 64 MB max per agent
    .instances(1)
    .tables(100)
    .build();

// CPU time via fuel or epoch interruption
let mut config = Config::new();
config.consume_fuel(true);          // instruction count limit
config.epoch_interruption(true);    // wall-clock time limit
```

**Alternatives considered:**

| Runtime | License | Stars | Reason Not Primary |
|---|---|---|---|
| Wasmer v5 | MIT | 20,000 | No Component Model yet (planned); WASIX fragments ecosystem |
| WasmEdge | Apache-2.0 | 10,100 | No Component Model; good for AI inference at edge via WASI-NN |
| wasm3 | MIT | 7,700 | Interpreter-only; not suitable for server-side latency-sensitive workloads |
| Lunatic | Apache-2.0 OR MIT | 4,800 | Last release May 2023 — stalled development |

**When to use alternatives:**
- **WasmEdge**: If WASI-NN (TensorFlow Lite, OpenVINO) for edge AI inference is needed
- **Wasmer headless**: Stripped-down deployment with pre-compiled modules
- **Extism** (BSD-3-Clause, 5,200 stars): If you need a simple plugin host with 15+ language SDKs and don't need Component Model

---

### 3.5 WASM Plugin System — WIT Interfaces

**PRIMARY PICK:** WIT (WebAssembly Interface Types) + `wit-bindgen` v0.46.0 + `wasm-tools`

**Secondary option:** Extism v1.12.0 (BSD-3-Clause) for simpler plugin patterns without Component Model

**Justification:**
- WIT is the canonical IDL for the WebAssembly Component Model
- `wit-bindgen` generates type-safe host/guest bindings from `.wit` files for Rust, C, C#, Go
- `wasm-tools component wit ./agent.wasm` allows programmatic capability auditing of any compiled component — verifying what an agent can and cannot access before deployment
- wasmCloud's Platform Harness pattern (used by American Express) is built entirely on WIT
- Enables defense against OWASP Top 10 for LLMs at the bytecode level

**WIT toolchain:**
```toml
# Build dependencies
wit-bindgen = "0.46"
wasm-tools = "1"          # CLI tool for validation and inspection
cargo-component = "0.21"  # Experimental — pin version tightly
```

**Integration pattern with Wasmtime:**
```rust
wasmtime::component::bindgen!({
    path: "wit/agent-sandbox.wit",
    world: "agent-sandbox",
});
```

**wasmCloud integration:** For production multi-tenant agent orchestration across clouds and edges, wasmCloud v1.9.0 (Apache-2.0, CNCF Incubating) provides:
- Capability-based security (deny by default)
- wRPC (WIT over RPC) for distributed component communication over NATS
- SandboxMCP: generates sandboxed MCP servers from OpenAPI specs
- K8s-native in wasmCloud 2.0 (CRDs, ArgoCD, Helm)

---

### 3.6 Web API — Axum + Tower + Tonic

**PRIMARY PICK (REST/HTTP):** Axum v0.8.8 (MIT, 25,100 stars, 248M+ downloads)  
**PRIMARY PICK (gRPC):** Tonic v0.14.5 (MIT, 11,400 stars, 142.8M tonic-build downloads)  
**Middleware:** Tower v0.5.1 (MIT, 6,209 dependent crates) + tower-http v0.6.8

**Justification for Axum:**
- Tokio-team backing ensures long-term maintenance alignment with the async runtime
- Macro-free routing, composable Tower middleware, first-class SSE/WebSocket support
- Production users: docs.rs, Lichess, Vinted, Azure ML, iFood
- Native LLM token streaming via `Sse<impl Stream>` — the de facto pattern for OpenAI-compatible APIs
- ~10–15% slower than Actix-web in synthetic benchmarks; effectively zero difference in DB-bound real workloads

**Justification for Tonic:**
- Undisputed standard for Rust gRPC; no viable alternative at the same maturity level
- Axum router integration (`router` feature) enables mixed REST + gRPC on same port
- Bi-directional streaming for high-throughput internal pipelines (agent tool calls)

**Full web stack:**
```toml
axum = { version = "0.8", features = ["ws", "macros"] }
axum-extra = { version = "0.10", features = ["typed-routing", "cookie", "protobuf", "json-lines"] }
tower = "0.5"
tower-http = { version = "0.6", features = [
    "cors", "trace", "compression-full", "auth", "timeout",
    "request-id", "normalize-path", "sensitive-headers", "catch-panic"
] }
tonic = { version = "0.14", features = ["transport", "tls-ring", "gzip", "router"] }
prost = "0.14"
reqwest = { version = "0.13", features = ["json", "stream"] }
reqwest-eventsource = "0.6"    # Consuming upstream LLM SSE streams
utoipa = { version = "5", features = ["axum_extras"] }
utoipa-axum = "0.1"
```

**LLM streaming pattern:**
```rust
// Server-side: stream LLM tokens as SSE
async fn chat_completions(State(state): State<AppState>) 
-> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.llm_broadcast.subscribe();
    Sse::new(try_stream! {
        loop {
            match rx.recv().await {
                Ok(token) => yield Event::default().event("token").data(token),
                Ok(_done) => { yield Event::default().event("done"); break; }
                Err(_) => break,
            }
        }
    }).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
```

**Alternatives considered:**
- **Actix-web** v4.13.0 (MIT/Apache, 24,500 stars): Better raw throughput but more complex architecture; use if extreme throughput (100k+ RPS) becomes a requirement
- **Poem**: Good for OpenAPI-first with `poem-openapi`; lacks Tower ecosystem breadth
- **Salvo**: Only Rust framework with HTTP/3; use if HTTP/3 becomes a requirement

---

### 3.7 Frontend — Leptos (SSR) or Hybrid TypeScript

**PRIMARY PICK:** Leptos v0.8.11 (MIT, 19,300 stars) with SSR + hydration  
**Alternative:** Keep TypeScript frontend (React/Next.js) and expose REST/WebSocket API only

**Justification for Leptos:**
- Best SSR story in the Rust ecosystem: streaming HTML via `<Suspense>`, server functions (`#[server]`), code splitting via `#[lazy_route]` (summer 2025)
- Performance benchmark (Krausest JS Framework Benchmark): **~1.18× overhead vs vanilla JS** — beats React, Angular, Vue, Alpine
- Server-side: **~10× more requests/second** vs Remix/Express.js on same hardware
- Fine-grained reactivity (no VDOM), signals as primitive, `Send + Sync` for Axum/Tokio
- Code splitting reduces initial bundle to ~413KB for medium apps

**When to choose Dioxus instead:**
- Cross-platform (web + desktop + mobile) from one codebase
- Team prefers React-like mental model (hooks, `rsx!` macro)
- Ant Design component library (adui-dioxus, experimental) needed

**When to keep TypeScript frontend:**
- Existing React codebase with significant investment
- Team lacks Rust frontend experience
- Design system relies heavily on JS-native libraries (D3, complex charts)
- Timeline pressure: Leptos learning curve adds ~2–4 weeks per engineer

**Build pipeline:**
```
cargo-leptos serve          # Full-stack dev with SSR + WASM hot reload
cargo-leptos build --split  # Production: optimized WASM with code splitting
trunk build --release       # If CSR-only
wasm-pack build             # For npm-compatible WASM libraries
```

---

### 3.8 Database & Vector Store — SQLx/SeaORM + Qdrant

**PRIMARY PICK (relational):** SQLx v0.8.x (MIT/Apache, 15,900 stars) for raw queries + SeaORM v1.x for ORM features  
**PRIMARY PICK (vector):** Qdrant + `qdrant-client` v1.17.0 (Apache-2.0)

**Justification for SQLx:**
- Compile-time SQL query verification (`query!` macro catches SQL errors at compile time)
- Pure Rust async Postgres driver (zero C FFI for Postgres); MSSQL removed in 0.8
- `LISTEN`/`NOTIFY` for async PostgreSQL notifications (agent event streaming)
- 250k+ weekly downloads; production-used by SeaORM, Loco, and major commercial Rust services

**Justification for SeaORM:**
- Production stable at v1.0; used by Zed editor, OpenObserve, RisingWave, LLDAP, Warpgate
- Built on SQLx — inherits all async/compile-time safety
- Seaography: automatic GraphQL API generation from entities
- Active model pattern tracks which fields are explicitly set (prevents partial-update bugs)

**Justification for Qdrant:**
- Leading Rust-first vector database (21,000+ GitHub stars for Qdrant server)
- Official Rust client; gRPC transport for throughput
- Sub-10ms p99 at millions of vectors; scalar and product quantization for memory efficiency
- First-class support in Rig's RAG pipeline integration

**Connection pooling:**
```toml
sqlx = { version = "0.8", features = [
    "postgres", "sqlite", "runtime-tokio-rustls", "macros",
    "migrate", "uuid", "chrono", "json"
] }
sea-orm = { version = "1", features = ["sqlx-postgres", "runtime-tokio-rustls", "macros"] }
qdrant-client = "1"
deadpool-postgres = "0.14"     # OR bb8
```

**Row-level security for multi-tenancy:**
```sql
ALTER TABLE agent_runs ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON agent_runs
    USING (tenant_id = current_setting('app.current_tenant')::uuid);
```

**Alternative vector stores:**

| Option | Use Case |
|---|---|
| LanceDB v0.14 (Apache-2.0) | Embedded/serverless; no separate server; Arrow-native |
| pgvector v0.4 (MIT) | If Postgres is primary DB; HNSW indexes; ACID with relational data |
| usearch v2.21 (Apache-2.0) | Ultra-fast in-process ANN; **WASM-compatible** |
| ~~pgvecto.rs~~ / VectorChord | **DO NOT USE** — VectorChord successor uses AGPL/ELv2 |

---

### 3.9 Caching & Message Queue — moka + async-nats

**PRIMARY PICK (in-process cache):** moka v0.12.13 (MIT/Apache, 2,200 stars)  
**PRIMARY PICK (distributed cache/session):** redis v1.0.x + bb8-redis  
**PRIMARY PICK (message queue):** async-nats v0.46.0 (Apache-2.0, 1,300 stars)  
**Secondary (Kafka for external events):** rdkafka v0.25.x (MIT, 1,900 stars)

**Justification for moka:**
- Powers crates.io API with 85%+ cache hit rates; used in embedded Linux routers since 2021
- TinyLFU admission + LRU eviction; near-optimal hit ratio; fully async-native
- Both sync (`moka::sync::Cache`) and async (`moka::future::Cache`) variants
- TTL, TTI, per-entry variable expiration, weighted eviction, eviction listener callbacks
- ⚠️ Explicitly not WASM-compatible; use `mini-moka` (MIT/Apache) or `cached` (MIT) for WASM

**Justification for async-nats:**
- Official NATS Rust client from the NATS team; sub-millisecond latency
- JetStream: persistence, at-least-once, exactly-once delivery
- KV store + Object store capabilities
- Pairs natively with wasmCloud's NATS lattice for agent-to-capability-provider communication
- Millions of messages/second throughput

**When to use rdkafka:**
- External event streaming integrations (bank feeds, document ingestion pipelines)
- Exactly-once semantics (EOS) required
- Integration with existing Kafka infrastructure

```toml
moka = { version = "0.12", features = ["future"] }
redis = { version = "1", features = ["tokio-comp", "cluster-async"] }
async-nats = "0.46"
rdkafka = "0.25"    # optional: external Kafka integration
```

---

### 3.10 Auth & Security — jsonwebtoken + casbin + RustCrypto

**PRIMARY PICK (JWT):** jsonwebtoken v10.3.0 (MIT, 6,500 stars, 100M+ downloads)  
**PRIMARY PICK (authorization):** casbin v2.20.0 (Apache-2.0, 1,000 stars)  
**PRIMARY PICK (TLS):** rustls v0.23.37 (Apache-2.0/ISC/MIT, 520M+ downloads)  
**PRIMARY PICK (crypto):** RustCrypto organization crates (all MIT/Apache-2.0)

**Auth architecture for multi-tenant platform:**

```
[External OIDC Provider (Azure AD / Okta / Keycloak)]
  ↓ OIDC Discovery + ID Token validation
[openidconnect v4.0.1] → [jsonwebtoken v10.3.0]
  ↓ JWT with {sub, tenant_id, roles, capabilities, exp}
[casbin v2.20.0] → RBAC with domain (tenant_id)
  ↓ Per-request policy enforcement in Axum middleware
[tower-governor v0.8.0] → per-tenant rate limiting
  ↓ Request reaches handler
[vaultrs / vault-client-rs] → per-tenant secrets
```

**Multi-tenant RBAC with Casbin:**
```
// model.conf
[role_definition]
g = _, _, _          // user, role, tenant

[matchers]
m = g(r.sub, p.sub, r.dom) && r.dom == p.dom && r.obj == p.obj && r.act == p.act
```

**API key structure:**
```
{prefix}_{tenant_id}_{random_bytes_base62}
// Example: "ak_01H9XYZ_7xK2mN3..."
// Store only Argon2id hash in DB; index on key_prefix for O(1) lookup
```

**Critical security choices:**
- `argon2` (Argon2id) for password hashing — PHC winner, memory-hard, pure Rust
- `orion` v0.17.13 (MIT) to replace `sodiumoxide` (archived 2022)
- `rcgen` for mTLS certificate generation per agent
- `webauthn-rs` (MPL-2.0) OR `passkey` (MIT/Apache) — prefer `passkey` to avoid MPL
- `ring` (Apache/ISC) for production crypto — 422M+ downloads, but difficult to cross-compile; use RustCrypto for WASM paths

**Security crates:**
```toml
jsonwebtoken = { version = "10", features = ["aws_lc_rs"] }
oauth2 = "5"
openidconnect = "4"
axum-login = "0.18"
tower-sessions = { version = "0.15", features = ["signed"] }
argon2 = "0.5"
casbin = { version = "2.20", features = ["runtime-tokio"] }
rustls = { version = "0.23", features = ["aws_lc_rs"] }
tokio-rustls = "0.26"
aes-gcm = "0.10"
sha2 = "0.10"
p256 = "0.13"
orion = "0.17"
rcgen = "0.14"
tower-governor = "0.8"
vaultrs = "0.7"
```

---

### 3.11 Observability — tracing + OpenTelemetry + Custom LLM Tracing

**PRIMARY PICK:** `tracing` ecosystem (MIT, 6,300+ stars) + OpenTelemetry OTLP → Langfuse

**Architecture principle:** Instrument once with `tracing::instrument`, export anywhere via layered subscriber.

**Full observability stack:**
```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"
tracing-opentelemetry = "0.28"
opentelemetry = "0.27"
opentelemetry-sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.27", features = ["tonic"] }
metrics = "0.23"
metrics-exporter-prometheus = "0.15"
tiktoken-rs = "0.6"
```

**LLM-specific span fields (OTel GenAI semantic conventions):**
```rust
#[instrument(
    name = "llm.call",
    fields(
        gen_ai.system = "openai",
        gen_ai.request.model = %model,
        gen_ai.usage.input_tokens = tracing::field::Empty,
        gen_ai.usage.output_tokens = tracing::field::Empty,
        gen_ai.usage.cost_usd = tracing::field::Empty,
    )
)]
async fn call_llm(client: &Client, model: &str, messages: &[Message]) -> Result<String> {
    // ... call LLM ...
    span.record("gen_ai.usage.input_tokens", usage.prompt_tokens);
    span.record("gen_ai.usage.cost_usd", estimate_cost(model, usage));
    // Record metrics too
    metrics::counter!("llm_requests_total", "model" => model).increment(1);
    metrics::histogram!("llm_latency_ms", "model" => model).record(latency);
}
```

**Langfuse via OTLP (recommended LLM observability):**
- Langfuse v3 accepts OTLP traces at `/api/public/otel` (HTTP/protobuf)
- Self-hostable (MIT), no Rust SDK required — use opentelemetry-otlp directly
- LLM-specific attributes: `langfuse.observation.type = "generation"`, input/output recording

**Key LLM metrics to track:**

| Metric | Type | Labels |
|---|---|---|
| `llm_requests_total` | Counter | model, provider, status |
| `llm_latency_ms` | Histogram | model, provider |
| `llm_ttft_ms` | Histogram | model (time-to-first-token) |
| `llm_input_tokens_total` | Counter | model, provider |
| `llm_output_tokens_total` | Counter | model, provider |
| `llm_cost_usd_total` | Counter | model, provider |
| `agent_iterations_total` | Counter | agent_id, status |
| `agent_tool_calls_total` | Counter | tool_name, status |
| `rag_retrieval_hits` | Counter | collection |

**AVOID:** `opentelemetry-jaeger` (DEPRECATED — use OTLP instead), `opentelemetry-prometheus` (DISCONTINUED — security vulnerability in protobuf dep)

---

### 3.12 MCP Protocol — rmcp

**PRIMARY PICK:** `rmcp` v0.16.0 (MIT, 2,500 GitHub stars)

**Justification:**
- Official Rust MCP SDK under the `modelcontextprotocol` GitHub organization (Anthropic-owned)
- All transport modes supported: stdio, SSE server/client, Streamable HTTP (2025-11-25 spec), child process
- Protocol versions: 2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25, `draft`
- `#[tool]` and `tool_router!` macros for zero-boilerplate tool definitions
- OAuth2 support; schemars integration for automatic JSON Schema generation

**MCP server example:**
```rust
rmcp = { version = "0.1", features = [
    "server", "client", "macros",
    "transport-io", "transport-sse-server", "transport-streamable-http-server",
    "schemars",
] }
```

**Transport selection:**
- **stdio**: Local tool servers, subprocess model, development
- **Streamable HTTP**: Preferred new transport for production shared services (2025-11-25 spec)
- **SSE**: Legacy remote servers; still widely supported

---

### 3.13 Serialization & Config — serde ecosystem

**PRIMARY PICK:** serde v1.0.228 (MIT/Apache, 43.2M/month, 956k dependent crates)

**Format selection guide:**

| Format | Crate | Use Case |
|---|---|---|
| JSON | `serde_json` v1.0.149 (MIT/Apache, 44.9M/month) | All API boundaries, LLM I/O, tool schemas |
| TOML | `toml` v1.0.3 (MIT/Apache, 31.2M/month) | Config files (preferred over YAML) |
| Binary | `postcard` v1.1.3 (MIT/Apache, 2.1M/month) | Internal agent messages, WASM contexts |
| Protobuf | `prost` v0.14.3 (Apache-2.0, 25.5M/month) | gRPC, typed cross-service schemas |
| MessagePack | `rmp-serde` v1.3.1 (MIT) | Binary with cross-language compatibility |

**Configuration layering:**
```toml
config = "0.15"    # Primary: layered 12-factor config
dotenvy = "0.15"   # Dev only: .env loading
```

**Schema & validation:**
```toml
schemars = "1"     # JSON Schema generation for MCP tools, OpenAPI, LLM function calling
garde = { version = "0.22", features = ["derive"] }  # Context-aware validation
```

**AVOID:** `bincode` (RUSTSEC-2025-0141, UNMAINTAINED), `serde_yaml` (deprecated by author), `dotenv` (RUSTSEC-2021-0141)

---

### 3.14 Testing & CI/CD — cargo-nextest + cargo-deny + sccache

**PRIMARY PICK (test runner):** cargo-nextest v0.9.106 (MIT/Apache, 2,600 stars) — 3–5× faster than `cargo test`  
**PRIMARY PICK (security/license gates):** cargo-deny v0.18.5 (MIT/Apache, 2,100 stars)  
**PRIMARY PICK (build cache):** sccache v0.12.0 (Apache-2.0, 6,700 stars) — 2–5× build time reduction  
**PRIMARY PICK (Docker):** cargo-chef v0.1.72 (MIT/Apache, 2,300 stars) — Docker layer caching

**Full CI toolchain:**
```toml
[dev-dependencies]
mockall = "0.14"
wiremock = "0.6"
rstest = "0.26"
proptest = "1.10"
insta = "1.46"
testcontainers = "0.25"
criterion = { version = "0.8", features = ["html_reports"] }
fake = "4.4"
test-log = { version = "0.2", features = ["trace"] }
```

---

## 4. Architecture Diagram (ASCII)

```
╔══════════════════════════════════════════════════════════════════════════════╗
║                    EXTERNAL CLIENTS & PARTNERS                               ║
║  Browser (Leptos WASM) │ API Clients (REST/gRPC) │ MCP Clients │ Tax Tools   ║
╚════════════╤═══════════╧═══════════╤══════════════╧══════════════════════════╝
             │                       │
             │ HTTPS/WSS             │ gRPC + TLS / HTTP
             ▼                       ▼
╔════════════════════════════════════════════════════════════════════════════╗
║                     EDGE / TLS TERMINATION LAYER                          ║
║            Caddy / Nginx / AWS ALB (WAF, rate limiting, TLS)              ║
╚═══════════════════════════════════┬════════════════════════════════════════╝
                                    │
             ┌──────────────────────┴───────────────────────┐
             │                                              │
             ▼                                              ▼
╔════════════════════════╗                    ╔══════════════════════════╗
║      REST API SERVER   ║                    ║     gRPC SERVER          ║
║  Axum 0.8 + Tower      ║                    ║     Tonic 0.14           ║
║  ┌─────────────────┐   ║                    ║  ┌──────────────────┐   ║
║  │ Tower Middleware│   ║                    ║  │ Auth Interceptor │   ║
║  │ - CORS          │   ║                    ║  │ Tracing Layer    │   ║
║  │ - Auth (JWT)    │   ║                    ║  │ Compression      │   ║
║  │ - Rate Limit    │   ║                    ║  └──────────────────┘   ║
║  │ - Tracing       │   ║                    ║  AgentService            ║
║  │ - Compression   │   ║                    ║  StreamingService        ║
║  └─────────────────┘   ║                    ╚══════════════════════════╝
║  /api/v1/agents        ║                                │
║  /api/v1/chat (SSE)    ║                    ┌───────────┘
║  /api/v1/tools         ║                    │
║  /metrics              ║                    │
╚═══════════╤════════════╝                    │
            │                                 │
            └──────────────┬──────────────────┘
                           │
                           ▼
╔══════════════════════════════════════════════════════════════════════════╗
║                        AGENT ORCHESTRATION LAYER                         ║
║                   Rig 0.31 / ADK-Rust 0.3 (Tokio runtime)               ║
║  ┌──────────────────────────────────────────────────────────────────┐   ║
║  │  AgentOrchestrator                                               │   ║
║  │  - Multi-agent coordination (sequential / parallel / graph)     │   ║
║  │  - Tool routing via rmcp (MCP protocol)                          │   ║
║  │  - Memory management (short-term: moka; long-term: Qdrant)      │   ║
║  │  - RAG pipeline (embedding → Qdrant → context injection)        │   ║
║  └──────────────────────────────────────────────────────────────────┘   ║
║                                                                          ║
║  LLM Provider Abstraction (genai + async-openai + ollama-rs)            ║
║  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌──────────────────────┐   ║
║  │  OpenAI   │ │Anthropic  │ │  Gemini   │ │  Local (mistral.rs)  │   ║
║  │ Claude    │ │  DeepSeek │ │  Groq     │ │  Ollama              │   ║
║  └───────────┘ └───────────┘ └───────────┘ └──────────────────────┘   ║
╚════════════════════════════╤═════════════════════════════════════════════╝
                             │
                             ▼
╔══════════════════════════════════════════════════════════════════════════╗
║                   WASM SANDBOX BOUNDARY                                  ║
║  ┌────────────────────────────────────────────────────────────────────┐ ║
║  │              Wasmtime 27.x Runtime Host                            │ ║
║  │                                                                    │ ║
║  │   ┌────────────────┐  ┌────────────────┐  ┌────────────────────┐  │ ║
║  │   │ TaxDiligence   │  │  DocExtractor  │  │  ComplianceCheck   │  │ ║
║  │   │ Agent.wasm     │  │  Agent.wasm    │  │  Agent.wasm        │  │ ║
║  │   │                │  │                │  │                    │  │ ║
║  │   │ WIT imports:   │  │ WIT imports:   │  │ WIT imports:       │  │ ║
║  │   │ - call-llm     │  │ - read-blob    │  │ - query-database   │  │ ║
║  │   │ - query-db     │  │ - call-llm     │  │ - call-llm         │  │ ║
║  │   │ - log-output   │  │ - log-output   │  │ - log-output       │  │ ║
║  │   │                │  │                │  │                    │  │ ║
║  │   │ NO IMPORTS:    │  │ NO IMPORTS:    │  │ NO IMPORTS:        │  │ ║
║  │   │ ✗ filesystem   │  │ ✗ network-raw  │  │ ✗ write-blob       │  │ ║
║  │   │ ✗ sockets      │  │ ✗ query-db     │  │ ✗ filesystem       │  │ ║
║  │   │ ✗ env-vars     │  │ ✗ env-vars     │  │ ✗ env-vars         │  │ ║
║  │   └────────────────┘  └────────────────┘  └────────────────────┘  │ ║
║  │                                                                    │ ║
║  │   Per-agent limits: 64MB memory, 30s wall-clock, fuel metering    │ ║
║  │   Linear memory isolation: Agent A ≠ Agent B memory space         │ ║
║  └────────────────────────────────────────────────────────────────────┘ ║
║                                                                          ║
║   Capability Providers (can be local or distributed via NATS/wRPC)      ║
║   ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   ║
║   │LLM Proxy │ │ Database │ │  Blob    │ │  Vault   │ │ Messaging│   ║
║   │(genai)   │ │(SQLx)    │ │ (S3/GCS) │ │ Secrets  │ │ (NATS)   │   ║
║   └──────────┘ └──────────┘ └──────────┘ └──────────┘ └──────────┘   ║
╚══════════════════════════════════╤═══════════════════════════════════════╝
                                   │
              ┌────────────────────┼─────────────────────┐
              │                    │                     │
              ▼                    ▼                     ▼
╔═════════════════╗  ╔══════════════════════╗  ╔══════════════════════╗
║  RELATIONAL DB  ║  ║    VECTOR STORE       ║  ║   CACHE / QUEUE      ║
║  PostgreSQL 16  ║  ║    Qdrant             ║  ║  Redis (sessions,    ║
║  SQLx 0.8       ║  ║    qdrant-client 1.17 ║  ║   distributed cache) ║
║  SeaORM 1.x     ║  ║    + pgvector         ║  ║  moka (in-process)   ║
║  Row-Level      ║  ║    for Postgres-only  ║  ║  NATS JetStream      ║
║  Security       ║  ║    deployments        ║  ║  (async-nats 0.46)   ║
╚═════════════════╝  ╚══════════════════════╝  ╚══════════════════════╝
                                   │
                                   ▼
╔══════════════════════════════════════════════════════════════════════════╗
║                      OBSERVABILITY PLANE                                 ║
║  tracing 0.1 (all code) → OpenTelemetry OTLP → OTel Collector          ║
║  ┌─────────────┐  ┌──────────────────┐  ┌──────────────────────────┐   ║
║  │  Jaeger     │  │   Prometheus     │  │  Langfuse (LLM traces)   │   ║
║  │  (traces)   │  │   (metrics)      │  │  via OTLP endpoint       │   ║
║  └─────────────┘  └──────────────────┘  └──────────────────────────┘   ║
║  metrics-exporter-prometheus → /metrics → Grafana dashboards            ║
╚══════════════════════════════════════════════════════════════════════════╝
```

---

## 5. WASM Agent Sandboxing Deep Dive

### 5.1 WIT Interface Design for Tax Diligence Agents

The security model begins at the interface definition layer. Every capability an agent can use must be explicitly declared in a WIT world. Anything not listed is structurally inaccessible — not just denied at runtime, but absent from the module's import table at the bytecode level.

**Core WIT package:**
```wit
package taxplatform:agent-capabilities@0.1.0;

// ─── Shared types ───────────────────────────────────────────────────────────
interface types {
    record llm-request {
        model: string,
        messages: list<tuple<string, string>>,  // (role, content)
        max-tokens: u32,
        temperature: float32,
    }

    record llm-response {
        content: string,
        input-tokens: u32,
        output-tokens: u32,
        stop-reason: string,
    }

    record db-query-result {
        columns: list<string>,
        rows: list<list<string>>,
        row-count: u32,
    }

    variant agent-error {
        unauthorized(string),
        quota-exceeded(string),
        timeout(string),
        invalid-input(string),
        provider-error(string),
    }
}

// ─── Tier 0: LLM inference only ─────────────────────────────────────────────
interface basic-llm {
    use types.{llm-request, llm-response, agent-error};
    call-llm: func(req: llm-request) -> result<llm-response, agent-error>;
    log-event: func(level: string, message: string);
}

// ─── Tier 1: Tax document read-only access ───────────────────────────────────
interface tax-readonly {
    use types.{db-query-result, agent-error};
    query-tax-records: func(
        tenant-id: string,
        entity-id: string,
        tax-year: u16
    ) -> result<db-query-result, agent-error>;
    read-document: func(
        doc-id: string
    ) -> result<list<u8>, agent-error>;
}

// ─── Tier 2: Document analysis with LLM + read access ───────────────────────
interface tax-analysis {
    use types.{llm-request, llm-response, db-query-result, agent-error};
    include basic-llm;
    include tax-readonly;
    get-regulatory-context: func(
        jurisdiction: string,
        tax-type: string
    ) -> result<string, agent-error>;
}

// ─── Tier 3: Full write access (privileged agents only) ─────────────────────
interface tax-privileged {
    include tax-analysis;
    write-findings: func(
        entity-id: string,
        findings: list<string>
    ) -> result<string, agent-error>;
    send-notification: func(
        recipient: string,
        subject: string,
        body: string
    ) -> result<unit, agent-error>;
}

// ─── WASM worlds (one per agent tier) ────────────────────────────────────────
world tier0-sandbox {
    import basic-llm;
    export execute: func(task-json: string) -> result<string, string>;
}

world tier1-analysis {
    import basic-llm;
    import tax-readonly;
    export execute: func(task-json: string) -> result<string, string>;
    // Explicitly NO write capabilities, NO network, NO filesystem
}

world tier2-full-analysis {
    import tax-analysis;
    import wasi:clocks/wall-clock@0.2.0;  // Allowed: wall clock only
    import wasi:random/random@0.2.0;       // Allowed: random numbers
    // NOT importing: wasi:filesystem, wasi:sockets, wasi:http
    export execute: func(task-json: string) -> result<string, string>;
}

world tier3-privileged {
    import tax-privileged;
    import wasi:clocks/wall-clock@0.2.0;
    import wasi:random/random@0.2.0;
    export execute: func(task-json: string) -> result<string, string>;
}
```

### 5.2 Capability Model (What Agents Can/Cannot Access)

**Permission tier matrix:**

| Capability | Tier 0 | Tier 1 | Tier 2 | Tier 3 |
|---|---|---|---|---|
| Call LLM | ✅ | ✅ | ✅ | ✅ |
| Log output | ✅ | ✅ | ✅ | ✅ |
| Read tax records (DB) | ❌ | ✅ | ✅ | ✅ |
| Read documents (blob) | ❌ | ✅ | ✅ | ✅ |
| Get regulatory context | ❌ | ❌ | ✅ | ✅ |
| Write findings | ❌ | ❌ | ❌ | ✅ |
| Send notifications | ❌ | ❌ | ❌ | ✅ |
| Raw network sockets | ❌ | ❌ | ❌ | ❌ |
| Filesystem access | ❌ | ❌ | ❌ | ❌ |
| Environment variables | ❌ | ❌ | ❌ | ❌ |
| Process spawning | ❌ | ❌ | ❌ | ❌ |

**Security guarantee:** These restrictions are enforced at the **bytecode validation** level by Wasmtime. An agent attempting to import `wasi:filesystem` without it being declared in its WIT world fails module validation before a single instruction executes. This is architecturally different from container-based sandboxing where seccomp/AppArmor rules are applied at syscall level — WASM denies at the import resolution level, before any code runs.

### 5.3 Hot-Loading Agents Without Restart

**Pattern 1: Stateless per-request instantiation (recommended for Phase 2)**

Each agent invocation instantiates a fresh WASM module. State lives in capability providers (Qdrant for memory, Redis for session, PostgreSQL for persistent data). Module replacement = upload new `.wasm` file; next invocation uses the new version. Zero-downtime by design.

```rust
// Host-side agent invocation
async fn invoke_agent(
    engine: &Engine,
    component_bytes: &[u8],
    task: AgentTask,
    capabilities: &CapabilitySet,
) -> Result<AgentResult> {
    let component = Component::from_binary(engine, component_bytes)?;
    let mut store = Store::new(engine, AgentContext {
        tenant_id: task.tenant_id.clone(),
        capabilities: capabilities.clone(),
    });
    
    // Apply resource limits
    let limits = StoreLimitsBuilder::new()
        .memory_size(64 * 1024 * 1024)  // 64 MB
        .build();
    store.limiter(|ctx| &mut ctx.limits);
    
    // Set time limit via epoch
    store.set_epoch_deadline(30);  // 30 epochs = ~30 seconds
    
    // Instantiate and call the agent
    let (bindings, _instance) = TaxAgent::instantiate_async(&mut store, &component, &linker).await?;
    let result = bindings.call_execute(&mut store, &serde_json::to_string(&task)?).await?;
    
    Ok(AgentResult::from_json(&result?)?)
}
```

**Pattern 2: Pre-compiled module caching**

Wasmtime supports serializing a compiled module to a `.cwasm` file for near-instant reinstantiation. Cache compiled components in memory or Redis for warm-path invocations:

```rust
// Pre-compile once, serialize to disk
let component = Component::from_binary(&engine, wasm_bytes)?;
let serialized = component.serialize()?;
tokio::fs::write("agent_v1.cwasm", &serialized).await?;

// Fast reinstantiation from pre-compiled artifact
let component = unsafe { Component::deserialize_file(&engine, "agent_v1.cwasm")? };
// Instantiation now takes microseconds, not milliseconds
```

**Pattern 3: wasmCloud hot-reload (for production orchestration)**

```bash
# Developer workflow
wash dev --manifest agent-manifest.yaml  # hot-swaps on file change
# State externalized to NATS KV + Qdrant = hot-swap safe
```

### 5.4 Resource Limits (Memory, CPU/Fuel, Time)

```rust
use wasmtime::{Config, Engine, Store, StoreLimitsBuilder};

fn create_sandboxed_engine() -> Engine {
    let mut config = Config::new();
    config.consume_fuel(true);         // Enable instruction counting
    config.epoch_interruption(true);   // Enable time-based interruption
    Engine::new(&config).unwrap()
}

fn create_agent_store<T>(engine: &Engine, state: T) -> Store<T> {
    let mut store = Store::new(engine, state);
    
    // Memory limits
    let limits = StoreLimitsBuilder::new()
        .memory_size(64 * 1024 * 1024)  // 64 MB max linear memory
        .instances(1)
        .tables(100)
        .build();
    
    // Fuel for CPU-intensive operations (not suitable for long LLM streams)
    // Use epoch_interruption for wall-clock time instead
    store.set_epoch_deadline(30);  // 30 x 1ms ticks = 30s max per invocation
    
    store
}

// Background thread increments epoch every 1ms
fn start_epoch_ticker(engine: Engine) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(1)).await;
            engine.increment_epoch();
        }
    })
}
```

**Resource limit defaults by tier:**

| Resource | Tier 0 | Tier 1 | Tier 2 | Tier 3 |
|---|---|---|---|---|
| Max memory | 32 MB | 64 MB | 128 MB | 256 MB |
| Wall-clock time | 30s | 60s | 120s | 300s |
| Fuel (pre-LLM compute) | 10M | 100M | 500M | 1B |
| Max instances | 1 | 1 | 1 | 1 |
| Max tables | 50 | 100 | 200 | 500 |

### 5.5 Security Model vs Docker Containers

| Security Dimension | Docker Container | WASM Component (Wasmtime) |
|---|---|---|
| Memory isolation | OS process separation | Per-module linear memory (structural) |
| Capability grant mechanism | seccomp/AppArmor (runtime syscall filter) | WIT import declaration (compile-time bytecode) |
| Lateral movement after RCE | Shell + tools available | No capabilities beyond declared WIT imports |
| Network access | iptables configurable | Explicit `wasi:sockets` import required |
| Secret injection | Env vars / mounted files | Capability provider at runtime (no in-module access) |
| Attack surface | Full Linux kernel syscall surface | 30-40 WASM spec primitives |
| Cold start | 50–500ms | 5 microseconds (Wasmtime AOT) |
| Density per node | ~100 containers per K8s node | 10,000s per host process |
| Granularity | Container-level | Per-interface, per-component |
| Supply chain | Any OS binary | Validated bytecode only |

**Key insight from Cosmonic:** If an LLM produces malicious code via prompt injection that executes in a WASM sandbox, the attacker gains only what the WIT world permits. "Living off the land" attacks are impossible — there is no `bash`, `curl`, `ls`, or any OS utility inside the WASM sandbox.

### 5.6 WIT Definitions for Tax Diligence Use Case

**Capability auditing before deployment:**
```bash
# Before deploying any agent component, audit its capabilities
wasm-tools component wit ./dist/tax-analysis-agent.wasm

# Output shows EXACTLY what the agent can import/export:
# package root:component;
# world root {
#   import taxplatform:agent-capabilities/basic-llm@0.1.0;
#   import taxplatform:agent-capabilities/tax-readonly@0.1.0;
#   import wasi:clocks/wall-clock@0.2.0;
#   import wasi:random/random@0.2.0;
#   export execute: func(task-json: string) -> result<string, string>;
# }

# If the component imports ANYTHING unexpected, halt deployment
```

**Per-tenant capability grants via JWT:**
```rust
#[derive(Debug, Serialize, Deserialize)]
struct AgentClaims {
    sub: String,              // agent_id
    tenant_id: String,        // tenant domain for Casbin RBAC
    agent_tier: u8,           // 0-3: maps to WIT world
    capabilities: Vec<String>, // explicit capability list
    exp: usize,
    iat: usize,
}
```

---

## 6. Cargo Workspace Structure

### 6.1 Proposed Monorepo Layout

```
tax-platform/
├── Cargo.toml                    # Workspace root
├── Cargo.lock                    # Single shared lockfile (commit to VCS)
├── deny.toml                     # cargo-deny: license + security policy
├── .cargo/
│   └── config.toml               # Linker (mold), sccache, target settings
├── crates/
│   │
│   ├── platform-core/            # Shared domain types, no external deps
│   │   ├── Cargo.toml            # no_std + alloc features available
│   │   └── src/
│   │       ├── models/           # AgentTask, TaxEntity, Finding types
│   │       ├── errors/           # Platform error hierarchy
│   │       └── traits/           # AgentExecutor, CapabilityProvider traits
│   │
│   ├── platform-llm/             # LLM provider abstraction layer
│   │   ├── Cargo.toml            # genai + async-openai + ollama-rs
│   │   └── src/
│   │       ├── providers/        # OpenAI, Anthropic, Gemini, Local
│   │       ├── streaming/        # Token streaming infrastructure
│   │       └── tracing/          # LLM span instrumentation
│   │
│   ├── platform-agents/          # Agent orchestration (Rig + ADK-Rust)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── orchestrator/     # Multi-agent coordination
│   │       ├── rag/              # Retrieval-augmented generation pipeline
│   │       ├── memory/           # Short-term (moka) + long-term (Qdrant)
│   │       └── tools/            # Built-in tool implementations
│   │
│   ├── platform-wasm/            # WASM runtime host + sandbox management
│   │   ├── Cargo.toml            # wasmtime + wit-bindgen
│   │   └── src/
│   │       ├── runtime/          # Wasmtime engine, store management
│   │       ├── capabilities/     # Capability provider implementations
│   │       ├── sandbox/          # Resource limits, fuel, epoch management
│   │       └── registry/         # Agent .wasm artifact registry
│   │
│   ├── platform-mcp/             # MCP server + tool registry
│   │   ├── Cargo.toml            # rmcp + schemars
│   │   └── src/
│   │       ├── server/           # MCP server with all transport modes
│   │       ├── tools/            # Tax-specific MCP tools
│   │       └── resources/        # Document resources via MCP
│   │
│   ├── platform-api/             # HTTP/gRPC API server
│   │   ├── Cargo.toml            # axum + tonic + tower-http
│   │   └── src/
│   │       ├── routes/           # REST route handlers
│   │       ├── grpc/             # gRPC service implementations
│   │       ├── middleware/       # Auth, rate limiting, tracing
│   │       ├── streaming/        # SSE + WebSocket handlers
│   │       └── openapi/          # utoipa OpenAPI spec generation
│   │
│   ├── platform-auth/            # Authentication + authorization
│   │   ├── Cargo.toml            # jsonwebtoken + casbin + openidconnect
│   │   └── src/
│   │       ├── jwt/              # Token issuance, validation, JWKS
│   │       ├── oidc/             # Multi-tenant OIDC client
│   │       ├── rbac/             # Casbin policy enforcement
│   │       └── api-keys/         # API key management
│   │
│   ├── platform-db/              # Database access layer
│   │   ├── Cargo.toml            # sqlx + sea-orm + qdrant-client
│   │   └── src/
│   │       ├── migrations/       # sqlx-cli migrations
│   │       ├── repositories/     # Repository pattern per domain entity
│   │       ├── vector/           # Qdrant integration + embedding pipeline
│   │       └── cache/            # moka + Redis abstractions
│   │
│   ├── platform-observability/   # Tracing + metrics + profiling
│   │   ├── Cargo.toml            # tracing + opentelemetry + metrics
│   │   └── src/
│   │       ├── init/             # Subscriber initialization
│   │       ├── llm/              # LLM-specific span instrumentation
│   │       └── metrics/          # Prometheus metrics definitions
│   │
│   ├── platform-frontend/        # Leptos SSR frontend
│   │   ├── Cargo.toml            # leptos + leptos_router
│   │   └── src/
│   │       ├── app/              # App component + routing
│   │       ├── pages/            # Page components
│   │       └── components/       # Reusable UI components
│   │
│   ├── agent-tax-diligence/      # Tax diligence WASM agent (guest code)
│   │   ├── Cargo.toml            # wasm32-wasip2 target; minimal deps
│   │   ├── wit/                  # WIT interface definitions
│   │   └── src/
│   │       ├── lib.rs            # WASM component entry point
│   │       └── analysis/         # Tax analysis logic (no_std-safe)
│   │
│   ├── agent-doc-extractor/      # Document extraction WASM agent
│   │   ├── Cargo.toml
│   │   └── src/
│   │
│   └── server/                   # Main binary: ties all crates together
│       ├── Cargo.toml
│       └── src/main.rs           # tokio::main, config, startup
│
├── wit/                          # Shared WIT interface definitions
│   ├── agent-capabilities.wit    # Capability tiers (see §5.1)
│   └── tax-types.wit             # Domain types
│
├── proto/                        # Protobuf definitions (for gRPC)
│   └── agent_service.proto
│
├── migrations/                   # SQLx database migrations
│   ├── 20260101_initial.sql
│   └── 20260201_add_vector_search.sql
│
├── config/
│   ├── default.toml              # Default config (all environments)
│   ├── development.toml          # Dev overrides
│   └── production.toml.example  # Production template
│
├── docker/
│   ├── Dockerfile                # cargo-chef multi-stage build
│   ├── Dockerfile.wasm           # WASM component build
│   └── docker-compose.yml        # Local dev: Postgres, Qdrant, NATS, Redis
│
└── .github/
    └── workflows/
        ├── ci.yml                # Main CI pipeline
        ├── release.yml           # cargo-release pipeline
        └── wasm-build.yml        # WASM artifact build and push
```

### 6.2 Workspace Root Cargo.toml

```toml
[workspace]
members = ["crates/*"]
resolver = "2"  # Required for correct feature unification

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.83"
authors = ["Tax Platform Engineering"]
license = "UNLICENSED"

[workspace.dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }
tokio-stream = "0.1"
futures = "0.3"
async-stream = "0.3"
async-trait = "0.1"
flume = "0.11"

# Web
axum = { version = "0.8", features = ["ws", "macros"] }
axum-extra = { version = "0.10", features = ["typed-routing", "json-lines"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["full"] }
tonic = "0.14"
prost = "0.14"
reqwest = { version = "0.13", features = ["json", "stream"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "1"
postcard = { version = "1", features = ["alloc"] }
schemars = "1"

# Database
sqlx = { version = "0.8", features = [
    "postgres", "sqlite", "runtime-tokio-rustls", "macros", "migrate", "uuid", "chrono", "json"
] }
sea-orm = { version = "1", features = ["sqlx-postgres", "runtime-tokio-rustls", "macros"] }
qdrant-client = "1"

# Auth
jsonwebtoken = { version = "10", features = ["aws_lc_rs"] }
casbin = { version = "2.20", features = ["runtime-tokio"] }
argon2 = "0.5"

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
opentelemetry = "0.27"
opentelemetry-sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.27", features = ["tonic"] }
tracing-opentelemetry = "0.28"
metrics = "0.23"

# WASM
wasmtime = { version = "27", features = ["component-model", "runtime", "async"] }

# MCP
rmcp = { version = "0.1", features = ["server", "client", "macros", "transport-io", "schemars"] }

# Utilities
anyhow = "1"
thiserror = "2"
uuid = { version = "1", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
garde = { version = "0.22", features = ["derive"] }
config = "0.15"
dotenvy = "0.15"
moka = { version = "0.12", features = ["future"] }
async-nats = "0.46"

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"

[profile.dev]
# Optimize dependencies even in dev builds (faster Qdrant/reqwest compilation)
[profile.dev.package."*"]
opt-level = 2

[profile.wasm-release]
inherits = "release"
opt-level = "z"      # Optimize for size in WASM
lto = true
panic = "abort"      # Reduces WASM size significantly
```

### 6.3 WASM Agent Crate Feature Flags

```toml
# crates/agent-tax-diligence/Cargo.toml
[package]
name = "agent-tax-diligence"
version.workspace = true
edition = "2021"

[lib]
crate-type = ["cdylib"]  # Required for WASM component

[features]
default = []

[dependencies]
wit-bindgen = "0.46"
serde = { version = "1", features = ["derive"], default-features = false }
serde_json = { version = "1", default-features = false, features = ["alloc"] }
postcard = { version = "1", features = ["alloc"], default-features = false }

# No tokio, no sqlx, no heavy deps in agent guest code
# All I/O goes through WIT capability interfaces
```

---

## 7. Critical Deprecation Warnings

The following crates/tools must **NOT** be used in new code. Existing usage must be migrated during Phase 1.

### 7.1 Security Advisories (RustSec)

| Crate | Advisory | Status | Replacement |
|---|---|---|---|
| `bincode` | **RUSTSEC-2025-0141** | UNMAINTAINED (Dec 2025) — final v3.0.0 has a compiler error in lib.rs | `postcard` (MIT/Apache, no_std, stable wire format) or `rkyv` (zero-copy) |
| `dotenv` | **RUSTSEC-2021-0141** | UNMAINTAINED since 2021 | `dotenvy` v0.15.7 (MIT, drop-in replacement) |
| `opentelemetry-prometheus` | Security advisory | DISCONTINUED — depends on unmaintained `protobuf` crate with security vulns | `metrics-exporter-prometheus` + `prometheus-client` |

### 7.2 Deprecated by Maintainers

| Crate | Status | Replacement |
|---|---|---|
| `serde_yaml` | **DEPRECATED** by dtolnay (Mar 2024) | Use TOML for config files. If YAML required: `serde_yaml_ng` or continue 0.9 short-term. **Avoid `serde_yml`** (suspicious fork with unnecessary deps). |
| `opentelemetry-jaeger` | **DEPRECATED** (final v0.22.0) | `opentelemetry-otlp` — Jaeger natively accepts OTLP on port 4317 |
| `sodiumoxide` | **ARCHIVED Sept 2022** | `orion` v0.17 (MIT, pure Rust, WASM-compatible) or RustCrypto crates |
| `langchain-rust` | **STALE** (last release Oct 2024) | Rig (4,600+ stars, active, same functionality) |
| `llm-chain` | **UNMAINTAINED** (last release May 2023) | Rig |
| `rustformers/llm` | **ARCHIVED June 2024** | `mistral.rs` v0.7.0 (MIT, 6,100 stars) for local inference |
| `oso` | **DEPRECATED** (embedded library; company pivoted to SaaS) | `casbin` for embedded Rust RBAC; OpenFGA for relationship-based AuthZ |

### 7.3 Never Use (Incorrect Crate Names / Superseded)

| Pattern | Problem | Correct Approach |
|---|---|---|
| `rllm` (graniet) | Now just a thin wrapper over `llm`; no benefit | Use `llm` (graniet) directly or `genai` |
| `swarm-rs` (keith666666) | 1 star, educational only, Nov 2024 last commit | Use Rig |
| `serde_yml` | Suspicious AI-generated fork with OpenSSL/figlet-rs dependencies | Use `serde_yaml_ng` or TOML |
| pgvecto.rs successor **VectorChord** | AGPL/ELv2 license — incompatible with commercial use without paid license | Use `pgvector` (MIT) or Qdrant |
| `connectrpc` (Rust) | 0.1, Oct 2024, unary only, no streaming | Use `tonic` + `tonic-web` for browser-compatible gRPC today |
| `langsmith-rust` | 0 stars, unofficial, unsupported | Use OTLP → Langfuse (official OTLP endpoint) |

### 7.4 Version Pinning Warnings

| Crate | Issue | Action Required |
|---|---|---|
| `cargo-component` | Explicitly unstable — upgrading may cause build errors | Pin exact version in deny.toml; test every upgrade manually |
| `wit-bindgen` | Pre-1.0; breaking API changes possible | Pin to minor version; read CHANGELOG before every upgrade |
| `argon2` | Use `0.5.x` stable, NOT `0.6.0-rc.x` in production | Explicitly specify `argon2 = "0.5"` in Cargo.toml |
| `adk-rust` | `0.3.1` is Feb 2026; low star count (148) — evaluate carefully | Consider secondary/optional framework status |
| `redis` crate | Historically BSD-3-Clause on some versions | Run `cargo license` to verify exact version license before production |

---

## 8. Performance Targets

Based on benchmark data from the research files, the following concrete performance targets are set for the platform.

### 8.1 API Layer Performance

| Metric | Target | Basis |
|---|---|---|
| REST API latency (p50, non-LLM) | < 5ms | Axum benchmark data; DB-bound at ~1–50ms |
| REST API latency (p99, non-LLM) | < 50ms | Realistic with SQLx Postgres |
| Throughput (non-LLM endpoints) | > 10,000 RPS per instance | Axum TechEmpower benchmark tier |
| WebSocket connections per instance | > 10,000 concurrent | Tokio + Axum architecture |
| gRPC streaming throughput | > 100,000 messages/sec | Tonic + hyper HTTP/2 |

### 8.2 Agent Framework Performance

| Metric | Target | Basis |
|---|---|---|
| Agent cold start time | < 10ms | Rust agent frameworks: 4ms measured (vs Python 54–138ms) |
| Agent throughput (tool-heavy) | > 4 RPS per instance | AutoAgents benchmark: 4.97 rps (Rig: 4.44 rps) |
| Peak memory per agent process | < 2GB | Rust agents: 1.0–1.1GB vs Python 4.7–5.7GB |
| LLM call latency (p50) | External provider bound | ~2–8 seconds for GPT-4o; not framework-controllable |
| LLM call latency (p99) | < 30 seconds | With timeout enforcement |
| Time to first token (TTFT) | < 1 second | Monitor via `llm_ttft_ms` histogram |

### 8.3 WASM Sandbox Performance

| Metric | Target | Basis |
|---|---|---|
| WASM module instantiation (cold) | < 50ms | Wasmtime: 5μs AOT; up to 50ms for Component Model |
| WASM module instantiation (warm cache) | < 2ms | Pre-compiled `.cwasm` serialization |
| WASM execution overhead vs native | < 15% | 2025 Wasmer/Wasmtime benchmarks: ~88–95% native |
| Max concurrent WASM instances per host | > 1,000 | 10,000s per host (Cosmonic/wasmCloud production data) |
| WASM module size (tax agent) | < 5MB | -Oz optimization + wasm-opt |
| WASM module size compressed | < 2MB | Brotli compression for HTTP delivery |

### 8.4 Vector Search Performance

| Metric | Target | Basis |
|---|---|---|
| Vector similarity search (p99) | < 10ms at 1M vectors | Qdrant benchmark data |
| RAG context retrieval latency | < 50ms (embedding + search) | Embedding ~20ms + search ~10ms |
| Embedding batch throughput | > 100 embeddings/sec | Provider-dependent; local mistral.rs for high volume |

### 8.5 Infrastructure Performance

| Metric | Target | Basis |
|---|---|---|
| PostgreSQL connection pool saturation | < 80% utilization | SQLx Pool with 20–50 max connections |
| Redis cache hit rate | > 80% | moka (in-process) + Redis (distributed) layered strategy |
| NATS message latency | < 1ms p99 | NATS design spec; sub-millisecond throughput |
| Docker image build time (cached) | < 2 minutes | cargo-chef + sccache on GitHub Actions |
| Docker image build time (cold) | < 15 minutes | Rust compilation time; cold cache |
| CI test run time | < 5 minutes | cargo-nextest parallelization; sccache |

### 8.6 Multi-Tenancy Performance

| Metric | Target | Basis |
|---|---|---|
| Tenant isolation overhead | < 5% of request time | Row-level security in Postgres; JWT validation |
| Rate limiting overhead | < 1ms | tower-governor GCRA algorithm |
| RBAC enforcement overhead | < 2ms | casbin in-memory policy cache |

---

## 9. Build & Deploy Pipeline

### 9.1 GitHub Actions CI Workflow

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

jobs:
  lint-and-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
          targets: wasm32-unknown-unknown, wasm32-wasip2

      - uses: mozilla-actions/sccache-action@v0.0.7

      - uses: rui314/setup-mold@v1

      - name: Format check
        run: cargo fmt --all -- --check

      - name: Clippy (all targets, all features)
        env:
          RUSTC_WRAPPER: sccache
          CARGO_INCREMENTAL: "0"
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: License & security audit
        uses: EmbarkStudios/cargo-deny-action@v2

  test:
    runs-on: ubuntu-latest
    needs: lint-and-check
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_PASSWORD: test
        ports: ["5432:5432"]
        options: --health-cmd pg_isready --health-interval 10s
      redis:
        image: redis:7
        ports: ["6379:6379"]
      qdrant:
        image: qdrant/qdrant:latest
        ports: ["6333:6333", "6334:6334"]
      nats:
        image: nats:latest
        ports: ["4222:4222"]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: mozilla-actions/sccache-action@v0.0.7
      - uses: rui314/setup-mold@v1
      - uses: taiki-e/install-action@nextest

      - name: Run tests
        env:
          RUSTC_WRAPPER: sccache
          CARGO_INCREMENTAL: "0"
          SCCACHE_GHA_ENABLED: "true"
          DATABASE_URL: postgres://postgres:test@localhost:5432/test
        run: cargo nextest run --profile ci --all-features

      - name: Run doc tests
        run: cargo test --doc --all-features

  coverage:
    runs-on: ubuntu-latest
    needs: test
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-llvm-cov
      - uses: taiki-e/install-action@nextest

      - name: Generate coverage
        run: cargo llvm-cov nextest --lcov --output-path lcov.info --all-features

      - uses: codecov/codecov-action@v4
        with:
          files: lcov.info
          fail_ci_if_error: false

  wasm-build:
    runs-on: ubuntu-latest
    needs: lint-and-check
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown, wasm32-wasip2
      - uses: mozilla-actions/sccache-action@v0.0.7

      - name: Install wasm-pack
        uses: jetli/wasm-pack-action@v0.4.0

      - name: Install cargo-component
        run: cargo install cargo-component --version 0.21.1 --locked

      - name: Build WASM frontend
        env:
          RUSTC_WRAPPER: sccache
          CARGO_INCREMENTAL: "0"
        run: cargo leptos build --release

      - name: Build WASM agent components
        env:
          RUSTC_WRAPPER: sccache
        run: |
          cargo component build --release --manifest-path crates/agent-tax-diligence/Cargo.toml
          cargo component build --release --manifest-path crates/agent-doc-extractor/Cargo.toml

      - name: Audit WASM capabilities
        run: |
          wasm-tools component wit ./target/wasm32-wasip2/release/agent_tax_diligence.wasm
          wasm-tools validate ./target/wasm32-wasip2/release/agent_tax_diligence.wasm

      - name: WASM size audit
        run: |
          cargo install twiggy
          twiggy top -n 20 ./target/wasm32-wasip2/release/agent_tax_diligence.wasm

      - name: Upload WASM artifacts
        uses: actions/upload-artifact@v4
        with:
          name: wasm-agents
          path: target/wasm32-wasip2/release/*.wasm

  benchmarks:
    runs-on: ubuntu-latest
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Run criterion benchmarks
        run: cargo bench -- --save-baseline main
      - name: Upload benchmark results
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: target/criterion/
```

### 9.2 Docker Strategy (cargo-chef Multi-Stage)

```dockerfile
# ─── Stage 1: Chef installer ────────────────────────────────────────────────
FROM rust:1.83-bookworm AS chef
RUN cargo install cargo-chef --version 0.1.72
RUN apt-get update && apt-get install -y mold clang protobuf-compiler
WORKDIR /app

# ─── Stage 2: Dependency planning ───────────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ─── Stage 3: Dependency cooking (cached layer — only rebuilds on Cargo changes)
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Use mold linker for fast linking
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang
ENV RUSTFLAGS="-C link-arg=-fuse-ld=/usr/bin/mold"
RUN cargo chef cook --release --recipe-path recipe.json

# ─── Stage 4: Build application code ────────────────────────────────────────
COPY . .
RUN cargo build --release --bin server

# ─── Stage 5: Build WASM agent components ───────────────────────────────────
FROM chef AS wasm-builder
RUN rustup target add wasm32-wasip2
RUN cargo install cargo-component --version 0.21.1 --locked
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target wasm32-wasip2 --recipe-path recipe.json
COPY . .
RUN cargo component build --release
# Optimize WASM with wasm-opt
RUN apt-get install -y binaryen
RUN for f in target/wasm32-wasip2/release/*.wasm; do wasm-opt -Oz "$f" -o "$f"; done

# ─── Stage 6: Minimal runtime image ─────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/server /app/server
COPY --from=wasm-builder /app/target/wasm32-wasip2/release/*.wasm /app/agents/
COPY config/ /app/config/
ENV RUST_LOG=info
WORKDIR /app
CMD ["./server"]
```

### 9.3 WASM Compilation Pipeline

```
Source (Rust, .rs)
    │
    ├─ cargo component build --release (agent WASM components)
    │       └── wasm32-wasip2 target
    │               └── Wasmtime adapter (WASI P1 → P2)
    │
    ├─ wasm-opt -Oz (Binaryen optimization)
    │       └── 20–50% size reduction
    │
    ├─ wasm-tools validate (bytecode validation)
    │
    ├─ wasm-tools component wit (capability audit)
    │       └── FAIL if unexpected imports found
    │
    └─ Upload to artifact registry (OCI or S3)
           └── Referenced by platform-wasm crate at runtime
```

**WASM artifact versioning strategy:**
- Each agent component is versioned independently from the host platform
- Semantic versioning: MAJOR.MINOR.PATCH where MAJOR = breaking WIT interface change
- Agents and their WIT interfaces are co-versioned
- Platform host must support all minor versions within a MAJOR version

### 9.4 Deployment Targets

| Environment | Deployment Pattern | Notes |
|---|---|---|
| Local development | `docker-compose up` + `cargo leptos serve` | PostgreSQL + Qdrant + Redis + NATS |
| Staging | Kubernetes + Helm + wasmCloud operator | wasmCloud CRDs for agent lifecycle |
| Production | Kubernetes (EKS/GKE) + ArgoCD GitOps | WASM agents as OCI artifacts |
| Edge (future) | Fermyon Spin on Akamai/Fastly | For latency-sensitive tool endpoints |

---

## 10. Risk Assessment & Gaps

### 10.1 Production-Ready vs Experimental

**Production-ready (use now):**
- Tokio 1.x, Axum 0.8, Tonic 0.14, Tower — battle-tested, Tokio-team backed
- SQLx 0.8, SeaORM 1.0 — stable APIs, production deployments at scale
- Rig 0.31 — most mature Rust agent framework; real production deployments
- Wasmtime 27.x with WASI 0.2 — Shopify, Fastly, DFINITY in production
- tracing + OpenTelemetry ecosystem — industry standard in Rust
- jsonwebtoken + rustls — hundreds of millions of downloads
- Qdrant — leading Rust-native vector database; official client

**Beta/production-with-care:**
- genai 0.5 — excellent breadth but pre-1.0; pin minor version
- ADK-Rust 0.3 — impressive features but 148 stars; treat as secondary framework
- rmcp 0.16 — official SDK but v0.x; API may change; pin version
- casbin 2.20 — Rust port of Go Casbin; production use in Rust is less documented
- cargo-component 0.21 — explicitly experimental; WASM Component Model tooling
- tracing-opentelemetry 0.28 — strict version pairing required with opentelemetry

**Experimental (do not use in production yet):**
- WASI 0.3 native async — Wasmtime 37+ experimental; wait for stable release (~Feb 2026)
- WASM multi-threading — Threads proposal ratified but not yet in WASI
- wasmCloud 2.0 Kubernetes integration — announced KubeCon NA 2025; evaluate for Phase 3+
- WIT async streams — part of WASI 0.3; not available in 0.2

### 10.2 WASI 0.3 Timeline Dependency

**Critical dependency:** The platform's WASM agent architecture in Phase 2 uses WASI 0.2 synchronous interfaces. Async I/O in WASM agents requires WASI 0.3.

**Current workaround:** All async I/O in agent code goes through capability provider calls (synchronous from the WASM guest's perspective; the host handles async dispatch). This adds latency for high-frequency tool calls but is correct and production-safe under WASI 0.2.

**WASI 0.3 timeline:**
- Wasmtime 37+ experimental support: available now (late 2025)
- WASI 0.3.0 final release: **~February 2026** (per wasi.dev roadmap — essentially now)
- WASI 0.3.x (cancellation, streams, threads): H1 2026
- WASI 1.0 (full stable guarantee): late 2026 / early 2027

**Action:** Plan Phase 3 architecture with WASI 0.3 native async in WASM agents. The API surface change is significant (WIT interfaces gain `async` keyword; Canonical ABI adds `stream<T>` and `future<T>`). Allocate a Sprint for the migration in Phase 3.

### 10.3 Hiring Considerations

**Market data (2025):**
- ~2.27 million global Rust developers; only ~709,000 use it as primary language
- 35% YoY increase in Rust job postings
- Average US salary: $130k; specialist salary: $180k–$230k; NYC premium: $212k average
- Stack Overflow 2025: 72% "admiration rate" — 9th year most-loved language

**Hiring strategy:**
- Budget 15–20% salary premium over equivalent Go/Python engineers
- Consider junior engineers who are learning Rust: retention is high (83% want to keep using it), and the learning curve selects for strong fundamentals
- Google reports Rust code needs 20% fewer revisions and 25% less code review time vs C++
- The WASM Component Model requires additional expertise beyond standard Rust

**Role-specific requirements:**
- **Platform engineer (WASM sandbox):** Needs Wasmtime internals, WIT interface design, Component Model. Rare — budget for training
- **Agent engineer (Rig/Tokio):** Standard Rust async + LLM API knowledge. More available
- **Frontend engineer (Leptos):** Rust + WASM + web knowledge. Consider keeping TypeScript frontend if this is a hiring blocker

### 10.4 Ecosystem Maturity Gaps

| Gap | Severity | Mitigation |
|---|---|---|
| No Rust equivalent of LangSmith/Langfuse | Medium | Use OTLP → Langfuse (official OTLP endpoint); sufficient for production monitoring |
| LLM cost tracking (no litellm equivalent) | Low | Build custom pricing table + tiktoken-rs; extract token counts from API responses |
| Component Model browser support | Medium | Server-side only for agent sandboxing; browsers get Leptos WASM (separate path) |
| Rust WASM debugger quality | Medium | DWARF source maps work in VS Code + LLDB; varies by runtime; plan debug builds in staging |
| No mature Rust GraphQL client | Low | REST APIs are sufficient; async-graphql for server-side if GraphQL needed |
| neo4rs pre-1.0 (Bolt 5.x incomplete) | Low-Medium | Use postgres + Apache AGE for knowledge graph if needed; neo4rs only for Neo4j-specific workloads |
| CNCF survey: 48% cite "lack of applicability" | Informational | Internal platform; applicability is clear; monitoring community perception for hiring |

### 10.5 Migration Path from TypeScript Prototype

**Assumed current state:** TypeScript/Node.js prototype with:
- Express or Fastify REST API
- OpenAI API calls via `openai` npm package
- PostgreSQL or MongoDB database
- React/Next.js frontend

**Recommended migration strategy:**

**Phase 1 (Months 1–3): Parallel backend**
- Build Rust backend (Axum + SQLx) alongside existing TypeScript
- Migrate read-only endpoints first; validate parity with integration tests
- Keep TypeScript frontend pointing at TypeScript backend initially
- Migrate database schema to PostgreSQL if not already there
- Add SQLx migrations; run both stacks against same database

**Phase 2 (Months 3–6): Agent core migration**
- Replace TypeScript LLM orchestration with Rig agents
- Introduce Qdrant for vector search (alongside existing DB)
- WASM sandbox for new agent types (not migrating existing TS agents yet)
- Frontend still on TypeScript; point at Rust API

**Phase 3 (Months 6–9): Full Rust**
- Migrate TypeScript agents to Rig / WASM components
- Leptos SSR frontend (or keep React/Next.js — team decision)
- Decommission TypeScript backend
- wasmCloud for production agent orchestration

**Key risks in migration:**
- JavaScript/TypeScript agent prompts and logic may not translate 1:1 to Rust; plan for re-testing
- TypeScript ecosystem has richer npm libraries for some domain-specific tasks (tax codes, IRS forms); evaluate custom WIT capability providers vs TS interop via Node.js sidecar
- Frontend migration from React to Leptos has high learning curve; consider hybrid approach

---

## 11. Phased Implementation Roadmap

### Phase 1: Foundation (Months 1–3)
**Goal:** Core Rust backend operational; team upskilled on Rust async; basic agent execution

**Deliverables:**

1. **Cargo workspace setup**
   - Initialize workspace structure (§6.1)
   - Configure cargo-deny (license policy), sccache, mold
   - Set up GitHub Actions CI (§9.1) with nextest + cargo-llvm-cov
   - Implement cargo-chef Docker build

2. **Database layer**
   - SQLx with migrations for core schema (tenants, agents, agent_runs, findings)
   - SeaORM entities for major domain models
   - Connection pooling (deadpool-postgres)
   - Row-level security policies for multi-tenancy

3. **Core API server**
   - Axum 0.8 with Tower middleware stack (CORS, tracing, rate limiting, auth)
   - JWT authentication (jsonwebtoken) + OIDC integration (openidconnect)
   - Casbin RBAC with tenant domain support
   - OpenAPI spec generation (utoipa)
   - Basic REST endpoints: agents CRUD, tenant management, runs

4. **Basic agent execution (without WASM)**
   - Rig 0.31 integration with OpenAI/Anthropic/Gemini providers
   - Simple ReAct-style agents for tax document analysis
   - LLM streaming via Axum SSE
   - Basic tool calling infrastructure

5. **Observability foundation**
   - tracing + OpenTelemetry OTLP setup
   - LLM span instrumentation (§3.11)
   - Prometheus metrics endpoint
   - Langfuse integration via OTLP

6. **MCP server**
   - rmcp 0.16 with stdio + SSE transports
   - Initial tax-domain tools: document query, entity lookup, regulatory context

**Exit criteria:** Can execute a tax diligence analysis agent via REST API; basic multi-tenancy; CI green; LLM costs tracked in Langfuse

---

### Phase 2: WASM Sandbox (Months 4–6)
**Goal:** Wasmtime integration with WIT interfaces; agent isolation; hot-loading

**Deliverables:**

1. **WIT interface design**
   - Define `taxplatform:agent-capabilities@0.1.0` WIT package (§5.1)
   - Implement 4 tier worlds (tier0 through tier3)
   - Generate Wasmtime host bindings via `wit-bindgen`

2. **Wasmtime runtime host**
   - `platform-wasm` crate with engine configuration
   - Per-agent resource limits (64MB memory, 30s epoch, fuel metering)
   - Capability provider implementations (LLM proxy, DB, Vault)
   - Module registry (compile once, cache `.cwasm` serialized form)
   - Hot-loading via stateless per-request instantiation

3. **WASM agent guest code**
   - `agent-tax-diligence` WASM component (Tier 2)
   - `agent-doc-extractor` WASM component (Tier 1)
   - cargo-component build pipeline
   - Capability audit in CI (wasm-tools component wit inspection)

4. **Security hardening**
   - API key management with Argon2id hashing (§3.10)
   - Vault integration for per-tenant secrets (vaultrs)
   - mTLS agent communication (rcgen for certificate generation)
   - WASM capability attestation chain

5. **Performance validation**
   - Criterion benchmarks for agent invocation overhead
   - WASM instantiation time measurement
   - Memory footprint comparison vs Phase 1

**Exit criteria:** Tax diligence agents execute in WASM sandbox; capability audit passes in CI; 1,000 concurrent agent instances validated; capability escalation attempt produces trap without affecting other agents

---

### Phase 3: Frontend Migration (Months 7–9)
**Goal:** Leptos SSR frontend OR validate hybrid TypeScript approach; full-stack type safety

**Decision point (Month 6):** Evaluate Leptos vs keeping TypeScript frontend based on:
- Team Rust frontend experience
- Design system requirements
- Timeline pressure
- Performance requirements

**If Leptos:**

1. **Leptos SSR frontend**
   - `platform-frontend` crate with cargo-leptos build
   - Server functions (`#[server]`) for type-safe RPC (no separate API layer for frontend)
   - Thaw UI component library for standard UI elements
   - Code splitting for initial bundle < 500KB
   - Agent dashboard: real-time SSE token streaming display

2. **Dioxus evaluation** (if cross-platform needed)
   - Evaluate for mobile/desktop admin tooling

**If hybrid (TypeScript + Rust API):**
- Ensure Axum API provides complete REST/WebSocket coverage
- TypeScript types generated from Rust OpenAPI spec (utoipa → TypeScript types via openapi-typescript)
- WebSocket integration for real-time agent output

**Regardless of frontend choice:**

3. **NATS JetStream agent coordination**
   - async-nats 0.46 for agent-to-agent message passing
   - Event-driven tax diligence workflows (sequential document processing)
   - Dead letter queues for failed agent invocations

4. **Advanced RAG pipeline**
   - Qdrant collection per tenant (namespace isolation)
   - Embedding pipeline with local mistral.rs for high-volume embeddings
   - Hybrid search (vector + BM25 full-text)

**Exit criteria:** End-to-end user workflow functional; agent results display in real-time; frontend performance meets targets

---

### Phase 4: Production Hardening (Months 10–12)
**Goal:** Multi-tenancy at scale; full observability; enterprise security; wasmCloud evaluation

**Deliverables:**

1. **Observability completeness**
   - Distributed tracing across all service boundaries
   - Custom LLM dashboards in Grafana (cost, latency, TTFT by model/tenant)
   - Alerting rules: LLM error rate, agent timeout rate, WASM trap rate
   - pprof CPU profiling integration for production load

2. **Multi-tenancy hardening**
   - PostgreSQL row-level security validation at scale
   - WASM capability isolation stress testing (concurrent multi-tenant workloads)
   - Tenant quota enforcement (LLM token budgets, agent invocation limits)
   - Audit logging for all capability invocations

3. **wasmCloud evaluation**
   - Evaluate wasmCloud 2.0 K8s integration for production agent orchestration
   - Compare with current Wasmtime-direct approach
   - If adopted: migrate agent registry to OCI artifacts; NATS lattice setup

4. **CI/CD maturity**
   - cargo-release for semantic versioning + changelog management
   - Multi-arch Docker builds (x86_64 + arm64)
   - Canary deployment pipeline (10% traffic → validation → 100%)
   - Automated rollback on error rate threshold

5. **Performance at scale**
   - Load test with k6: 10,000 concurrent users, 1,000 agents/hour
   - Database query optimization (EXPLAIN ANALYZE; index coverage)
   - WASM module pre-warming for predictable cold starts

6. **WASI 0.3 migration (if released)**
   - Upgrade WIT interfaces to use native `async` if WASI 0.3 stabilizes
   - Remove asyncify workarounds
   - Streaming LLM responses directly through WIT `stream<string>` type

**Exit criteria:** Platform handles 100 concurrent tenants; 99.9% uptime SLA validated; security audit passed; LLM cost dashboard operational; regulatory compliance documentation complete

---

## Appendix A: Quick Decision Reference

### Which database should I use?

- **Need SQL + compile-time safety:** SQLx 0.8 (raw) or SeaORM 1.0 (ORM)
- **Need vector search:** Qdrant (client/server) or pgvector (Postgres extension)
- **Need embedded vector search:** LanceDB (serverless, Apache-2.0)
- **Need graph traversal:** neo4rs (pre-1.0) or Postgres + Apache AGE extension
- **Never use:** pgvecto.rs (successor VectorChord is AGPL)

### Which serialization format?

- **API responses, LLM I/O:** serde_json
- **Config files:** TOML (not YAML — deprecated by author)
- **Internal binary messages:** postcard (replaces bincode)
- **gRPC schemas:** prost (protobuf)
- **Never use:** bincode (RUSTSEC-2025-0141), dotenv (RUSTSEC-2021-0141)

### Which auth approach?

- **JWT tokens:** jsonwebtoken v10 with aws_lc_rs feature
- **OIDC (SSO):** openidconnect v4 + oauth2 v5
- **RBAC with multi-tenancy:** casbin v2.20 with domain model
- **Password hashing:** argon2 v0.5 (Argon2id)
- **TLS:** rustls v0.23 (not OpenSSL)
- **Avoid:** webauthn-rs (MPL-2.0 — use passkey instead), sodiumoxide (archived 2022), oso (deprecated)

### WASM or not?

**Use WASM for:** Agent plugin code, untrusted/LLM-generated code, multi-tenant isolation, hot-loadable extensions

**Don't use WASM for:** The host runtime, database access, LLM API calls, file I/O heavy operations, anything requiring POSIX

---

## Appendix B: Dependency Version Pinning Policy

Given the rapidly evolving nature of many crates in this stack, the following pinning policy applies:

| Crate Category | Pinning Strategy |
|---|---|
| Stable, high-download crates (tokio, serde, axum) | Pin major version: `= "1"` |
| Pre-1.0 crates with API stability concerns | Pin minor version: `= "0.31"` |
| Explicitly experimental (cargo-component, wit-bindgen) | Pin exact version: `= "0.21.1"` |
| Security-critical (jsonwebtoken, rustls, argon2) | Pin minor version; subscribe to RUSTSEC alerts |
| LLM framework (rig-core, rmcp) | Pin minor version; test every minor upgrade |

**Tooling:** Run `cargo deny check` in CI on every PR. Configure `deny.toml` to block GPL/AGPL, flag RUSTSEC advisories, and alert on duplicate dependency versions.

---

## Appendix C: `deny.toml` Reference Configuration

```toml
# deny.toml — cargo-deny configuration
[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",      # Extism, OpenSSL components
    "ISC",               # ring, rustls
    "Unicode-DFS-2016",
    "Unicode-3.0",
    "MPL-2.0",           # webauthn-rs (if used), mdBook (docs only)
]
deny = [
    "GPL-2.0",
    "GPL-3.0",
    "AGPL-3.0",
    "LGPL-2.0",
    "LGPL-2.1",
    "LGPL-3.0",
]
copyleft = "warn"

[[licenses.clarify]]
name = "ring"
expression = "MIT AND ISC AND OpenSSL"
license-files = [{ path = "LICENSE", hash = 0xbd0eed23 }]

[advisories]
version = 2
db-urls = ["https://github.com/rustsec/advisory-db"]
ignore = [
    # Add acknowledged false positives here with explanation
    # "RUSTSEC-XXXX-XXXX",  # Reason: <explanation>
]

[bans]
multiple-versions = "warn"
wildcards = "deny"

# Explicitly banned crates
deny = [
    { name = "bincode", reason = "RUSTSEC-2025-0141: UNMAINTAINED — use postcard instead" },
    { name = "dotenv", reason = "RUSTSEC-2021-0141: UNMAINTAINED — use dotenvy instead" },
    { name = "sodiumoxide", reason = "ARCHIVED Sept 2022 — use orion instead" },
    { name = "openssl", reason = "Use rustls instead for memory safety and WASM compatibility" },
    { name = "oso", reason = "DEPRECATED by maintainer — use casbin instead" },
    { name = "vectorchord", reason = "AGPL/ELv2 license — incompatible without commercial license" },
]

[sources]
unknown-registry = "deny"
unknown-git = "warn"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

---

*This document is a living reference. Update version numbers and star counts from research sources with each quarterly review. All benchmark data is from research conducted March 1, 2026.*
