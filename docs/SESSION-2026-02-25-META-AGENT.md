# Session Documentation: Impulse Meta-Agent Architecture

**Date:** 2026-02-25
**Session:** Meta-Agent Architecture & Agent Context Integration
**Status:** In Progress

---

## Executive Summary

This session focused on transforming Impulse from a terminal multiplexer into a **meta-agent** that manages the cognitive state of AI coding agents. The core concept is Monitor-Inject-Extract-Refine (MIER):

- **Monitor**: Track what agents are doing (PTY output, tokens, intent)
- **Inject**: Push relevant context INTO agents when they need it
- **Extract**: Pull knowledge OUT of agents (decisions, files, errors)
- **Refine**: Summarize and distill cross-agent context

---

## Work Completed

### 1. Code Quality Improvements

| Change | Files | Description |
|--------|-------|-------------|
| Dead code elimination | `error.rs`, `storage/mod.rs` | Removed unused error variants and methods |
| Clippy fixes | Various | Fixed type complexity warnings |
| `#[must_use]` attributes | 15 files | Added compile-time safety for ignored returns |
| Test fix | `docs/cache.rs` | Fixed flaky timestamp test |
| Tests | - | **453 tests passing** |

### 2. Documentation Enhancement

| File | Description |
|------|-------------|
| `README.md` | Complete rewrite with product description, quick start, key bindings |
| `QUICKSTART.md` | New 5-minute step-by-step guide |
| `PLATFORMS.md` | New platform setup guide (macOS, Linux, Windows/WSL) |
| `docs/vision/INTENT-DETECTION-VISION.md` | AI-powered intent detection architecture |
| `docs/vision/REAL-TIME-INJECTION-VISION.md` | Real-time context injection architecture |
| `docs/ARCHITECTURE-CLARIFICATION.md` | Clarification on scaffolding vs dead code |

### 3. Agent Context Infrastructure

| Component | File | Description |
|-----------|------|-------------|
| Capabilities Manifest | `src/agent_discovery/mod.rs` | JSON manifest of Impulse tools |
| Environment Variables | `src/ui/terminal_pane.rs` | IMPULSE_SESSION_ID, IMPULSE_CAPABILITIES_PATH injected |
| Agent Discover Command | `src/main.rs` | `agent-discover` command to generate manifest |

### 4. Sub-Agents Created

| Agent | Purpose | File |
|-------|---------|------|
| `impulse-docs-agent` | Documentation clarity & Claude Code differentiation | `.opencode/agents/impulse-docs-agent.md` |
| `impulse-integration-agent` | Claude Code/OpenCode hook optimization | `.opencode/agents/impulse-integration-agent.md` |
| `impulse-verification-agent` | Quality gates & testing | `.opencode/agents/impulse-verification-agent.md` |
| `impulse-session-agent` | Session lifecycle management | `.opencode/agents/impulse-session-agent.md` |

### 5. TUI Enhancements (Via Sub-Agent)

| Enhancement | Description |
|-------------|-------------|
| Startup message | Banner sent to PTY when agent spawns with session info |
| Status bar | Shows platform info (Claude Code/OpenCode) and session ID |
| Context injection shortcut | Press 'i' to inject context into active pane |
| Ctrl+1/2/3 shortcuts | Spawn Claude Code/OpenCode/Codex with startup message |

### 6. CLAUDE.md Enhancement (Via Sub-Agent)

Updated with:
- Clear differentiation: Impulse (sidecar) vs Claude Code (agent)
- How they work together (hook-based integration)
- Environment variables table expanded
- Accessing capabilities section
- Session tracking workflow

---

## Work in Progress (Not Yet Wired)

These modules are scaffolding - ready to wire but not yet connected:

| Module | Status | Notes |
|--------|--------|-------|
| `src/agent/types.rs` | Scaffolding | AgentConfig, AgentType - not wired to TUI |
| `src/agent/cli.rs` | Scaffolding | CliAgent - not wired to TUI |
| `src/agent/factory.rs` | Scaffolding | UnifiedAgent trait - not wired |
| `src/intent/` | Scaffolding | Intent detection - not wired to PTY |
| `src/mcp/` | Scaffolding | MCP server - not wired |

| File | Description |
|------|-------------|
| `src/intent/types.rs` | Intent data structures (AgentIntent, IntentCategory, Activity) |
| `src/intent/detector.rs` | Rule-based classifier for fast intent detection |
| `src/intent/providers.rs` | AI provider abstractions (Claude, OpenAI, Minimax) |
| `src/intent/mod.rs` | Intent store and engine with conflict detection |

**Intent Categories:**
- Planning ("I'm going to...")
- Reading ("Looking at...")
- Action ("I need to...")
- Error Detection
- Decision Making

### 5. Flexible Agent Architecture (NEW - SCAFFOLDING)

**Status:** Scaffolding - not yet wired to rest of codebase

| File | Description |
|------|-------------|
| `src/agent/types.rs` | AgentConfig with builder pattern, AgentType, AgentBackend |
| `src/agent/cli.rs` | CliAgent for spawning Claude Code/OpenCode as subprocess |
| `src/agent/factory.rs` | UnifiedAgent trait, AgentManager registry |

**⚠️ Important:** These files are NOT yet connected to the TUI or other modules. They provide the architecture for:
- Switching between CLI agents (Claude Code, OpenCode) and API agents (Anthropic, OpenAI, Minimax)
- Unified agent management

**Wiring plan:**
- Replace `spawn_terminal()` with factory-based approach
- Connect to C/X/O key handlers
- Add bidirectional communication

---

## Architecture: Monitor-Inject-Extract-Refine

```
┌─────────────────────────────────────────────────────────────────┐
│                      IMPULSE META-AGENT                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │  MONITOR    │→ │  INJECT     │→ │  EXTRACT    │→│  REFINE   │
│  ├─────────────┤  ├─────────────┤  ├─────────────┤  ├─────────┤
│  │ PTY Parser  │  │ Threshold   │  │ Hook Handler│  │Synthesiz │
│  │ Intent Det  │  │ PTY Writer  │  │ File Parser │  │Cross-    │
│  │ Token Est   │  │ Real-time   │  │ Decision    │  │Agent     │
│  │ Activity    │  │ Injection   │  │ Extract     │  │Learning  │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └────┬────┘
│         │                 │                 │                │
│         ▼                 ▼                 ▼                ▼
│  ┌─────────────────────────────────────────────────────────┐  │
│  │              AGENT CACHE (In-Memory)                     │  │
│  │  - Active sessions      - Intent hypothesis             │  │
│  │  - Current context %   - Recent extractions           │  │
│  └─────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Current Capabilities Status

| Function | Status | Notes |
|----------|--------|-------|
| **Monitor** | | |
| PTY output reading | ✅ | Counts bytes/lines |
| Activity tracking | ✅ | Active/Idle/Waiting states |
| Token estimation | ✅ | Stewardship module |
| Intent detection | 🔄 In Progress | Rule-based done, AI integration pending |
| **Inject** | | |
| Injection engine | ✅ | For daemon chat, orchestrate |
| PTY injection | ❌ Not wired | Vision doc created |
| Real-time triggers | ❌ Not wired | Vision doc created |
| **Extract** | | |
| File tracking | ✅ | track-write |
| Tool tracking | ✅ | track-tool |
| Decision extraction | ❌ Not implemented | - |
| **Refine** | | |
| Cross-agent synthesis | ❌ Not implemented | - |

---

## Key Files

### Core Modules
- `src/main.rs` - CLI entry, 50+ commands
- `src/ui/mod.rs` - TUI rendering (2443 lines)
- `src/state/mod.rs` - In-memory state (1515 lines)

### New Modules
- `src/intent/` - Intent detection (4 files)
- `src/agent/types.rs` - Flexible agent config
- `src/agent/cli.rs` - CLI agent spawning
- `src/agent/factory.rs` - Unified agent factory
- `src/agent_discovery/` - Capabilities manifest
- `src/notification/` - Event bus for agents

### Documentation
- `README.md` - Product overview, quick start
- `QUICKSTART.md` - 5-minute guide
- `PLATFORMS.md` - Platform setup
- `docs/vision/INTENT-DETECTION-VISION.md`
- `docs/vision/REAL-TIME-INJECTION-VISION.md`

---

## Metrics

| Metric | Value |
|--------|-------|
| Source files | 99 .rs |
| Lines of code | ~31,300 |
| Modules | 27 |
| Tests | 460 passing |
| Dynamic tools | 23 |
| Release binary | ~8.5MB |

---

## Next Steps

### Priority 1: Wiring
- [ ] Connect intent detection to PTY output parsing
- [ ] Wire injection to PTY sessions
- [ ] Connect stewardship thresholds to injection triggers

### Priority 2: Agent Integration
- [ ] Wire CLI agent spawning to TUI (C/X/O keys)
- [ ] Implement API agent integration
- [ ] Add bidirectional communication

### Priority 3: Advanced Features
- [ ] Real-time injection pipeline
- [ ] Decision extraction from agent output
- [ ] Cross-agent pattern detection

---

## Commands Reference

### Agent Management
```bash
impulse-rs agent-discover        # Generate capabilities manifest
impulse-rs session-start -n my-project -p claude-code
```

### Intent & Injection
```bash
impulse-rs orchestrate --task "add login" --inject-mode review
impulse-rs handoff --tool opencode --task "fix bug"
impulse-rs sync-context --session-id <id>
```

### Context & Memory
```bash
impulse-rs search-history --query "auth implementation"
impulse-rs search-genome --query "architecture decisions"
impulse-rs genome
impulse-rs history
```

---

## Questions for Future Sessions

1. **Which provider to prioritize for API integration?**
   - A) Anthropic (Claude)
   - B) OpenAI (GPT)
   - C) Minimax

2. **Real-time injection trigger preference?**
   - A) Token threshold (stewardship)
   - B) Manual trigger (/inject)
   - C) Agent request

3. **Scope for next session?**
   - A) Complete Monitor phase
   - B) Complete Inject phase
   - C) Both

---

*Last updated: 2026-02-25*
