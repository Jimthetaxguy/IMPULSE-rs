# Agent Architecture Clarification

**Date:** 2026-02-25
**Status:** Architecture in progress

---

## The Concern

Some agents are concerned about "dead code" in the agent-related modules. This document clarifies what's actually wired vs. what's scaffolding.

---

## What's ACTUALLY Used

### `src/agent/` - LLM Provider Module

| File | Status | Used By |
|------|--------|---------|
| `mod.rs` | ✅ Wired | main.rs, daemon/mod.rs |
| `anthropic.rs` | ✅ Wired | daemon/mod.rs (for chat) |

This module provides the LLM provider trait and implementations (Anthropic, OpenAI, Minimax) that power the daemon chat feature.

```rust
// Used in daemon/mod.rs
use crate::agent::{AnthropicProvider, ChatRequest, LlmProvider, Message, Role};
let provider = AnthropicProvider::new(api_key);
let response = provider.chat(request).await;
```

---

## What's SCAFFOLDING (Not Yet Wired)

### `src/agent/types.rs`, `cli.rs`, `factory.rs`

**Status:** Scaffolding - ready to wire, not yet connected

These files were created to support the flexible agent architecture vision:
- `types.rs` - AgentConfig, AgentType, AgentBackend
- `cli.rs` - CliAgent for spawning CLI tools (Claude Code, OpenCode)
- `factory.rs` - UnifiedAgent trait, AgentManager

**Why not wired yet:**
- The TUI currently spawns agents directly via `CommandBuilder::new("claude")`
- These files provide a more structured abstraction layer
- They're ready for future integration

**Planned wiring:**
- Connect to TUI spawn logic
- Replace direct command spawning with factory-based approach
- Enable both CLI and API agent modes

---

### `src/agent_discovery/`

**Status:** Scaffolding - capabilities manifest

This module generates a JSON manifest of Impulse's capabilities:
- Available tools (session_query, memory_search, etc.)
- Platform routing rules
- Feature flags

**Current state:**
- Command exists: `impulse-rs agent-discover`
- Generates: `.impulse/impulse-capabilities.json`
- Not automatically read by agents (they don't know to look for it)

**Planned wiring:**
- Inject manifest path into agent environment
- Add to agent startup context
- Agents can read it to discover Impulse tools

---

### `src/intent/`

**Status:** Scaffolding - intent detection module

Created for the Monitor-Inject-Extract-Refine architecture:
- `types.rs` - Intent data structures
- `detector.rs` - Rule-based classifier
- `providers.rs` - AI provider abstractions
- `mod.rs` - Intent store and engine

**Not wired yet:**
- PTY output parser doesn't call intent detector
- No real-time intent tracking

**Planned wiring:**
- Connect to PTY output thread
- Run intent detection on agent output
- Use intent for proactive injection

---

### `src/mcp/`

**Status:** Scaffolding - MCP server

Model Context Protocol server implementation.

**Not wired yet.**

---

### `src/plugin/`

**Status:** Unknown - needs investigation

Declared in main.rs but may not exist or may be incomplete.

---

## Architecture Plan

### Phase 1: Wire What's Ready

1. **Agent Discovery** → Agent startup context
   - Inject IMPULSE_CAPABILITIES_PATH
   - Add to startup message

2. **Intent Detection** → PTY output
   - Add callback to PTY reader
   - Parse output for intent patterns

### Phase 2: Agent Integration

1. **Factory** → TUI spawn
   - Replace direct CommandBuilder with factory
   - Support both CLI and API modes

2. **Context Injection** → Real-time
   - Wire stewardship thresholds to injection
   - Add PTY input writing

### Phase 3: Advanced Features

1. Decision extraction from agent output
2. Cross-agent pattern detection
3. Proactive context injection

---

## Why This Isn't "Dead Code"

"Dead code" implies code that serves no purpose and should be deleted. These modules are:

1. **Intentionally created** - For the Monitor-Inject-Extract-Refine vision
2. **Architecturally sound** - Well-designed interfaces and types
3. **Ready to wire** - Not broken, just not connected
4. **Documented** - Vision docs explain the purpose

The difference:
- **Dead code**: Unused and useless → delete
- **Scaffolding**: Unused but purposeful → wire in later

---

## Verification Commands

```bash
# Check which agent files are used
grep -r "use.*agent::" src/*.rs src/**/*.rs | grep -v "agent/"

# Check module declarations
grep "pub mod agent" src/main.rs

# Check agent_discovery usage
grep -r "agent_discovery" src/*.rs src/**/*.rs
```

---

## Decision Needed

Options for the scaffolding:

1. **Keep as-is** - Continue with wiring in future sessions
2. **Feature flag** - Wrap in `#[cfg(feature = "agent-factory")]`
3. **Remove temporarily** - Delete and recreate when needed
4. **Document as experimental** - Mark clearly in code

**Recommendation:** Keep as-is with clear documentation. The architecture is sound and the wiring plan is clear.

---

*Last updated: 2026-02-25*
