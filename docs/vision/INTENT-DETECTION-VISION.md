---
title: Intent Detection Vision
description: AI-powered intent detection for Impulse agent orchestration
version: '1.0'
updated: 2026-02-25
type: vision
category: core
phase: 3
status: active
audience: builders
tags: [vision, intent-detection, ai, orchestration, agents]
authors:
  - name: Impulse Maintainers
    role: Maintainer
    email: impulse-rs@users.noreply.github.com
    github: Jimthetaxguy/IMPULSE-rs
---

# Intent Detection Vision

> **Vision:** Impulse agents (Claude, GPT, Minimax, etc.) will have AI-powered intent detection to understand what other agents are working on, enabling smarter coordination and context injection.

---

## Current State

The current Impulse architecture supports:
- Session tracking and lifecycle management
- File and tool activity tracking
- Context injection via hooks (review-first mode)
- Basic pattern detection for coordination

**What's missing:** Real-time understanding of agent intent beyond pattern matching.

---

## Target State

### Core Concept: Intent Detection

Intent detection is the ability to understand **what an AI agent is trying to accomplish** from its actions, outputs, and context—not just pattern matching on file names or tool calls.

**Why it matters:**
- Pattern matching detects **overlap** (two agents editing same file)
- Intent detection predicts **coordination needs** (one agent should handle X while another handles Y)
- Enables proactive coordination before conflicts arise

### Supported Agent Types

| Agent | Intent Detection Method | Status |
|-------|------------------------|--------|
| Claude Code | MCP hooks + stdout parsing | Planned |
| Codex | Hook events + stdout parsing | Planned |
| OpenCode | Hook events + stdout parsing | Planned |
| Minimax | API-based intent queries | Planned |
| GPT (external) | API-based intent queries | Planned |
| Shell/Manual | Activity-based inference | Planned |

---

## Intent Detection Architecture

### Layer 1: Activity Capture

```
Agent Action → Hook/Event → Intent Extractor
```

**Capture methods:**
1. **MCP Hooks** (Claude Code): `on_tool_call`, `on_result`, `on_progress`
2. **Stdout/Stderr Parsing**: Parse agent output for intent signals
3. **File Activity**: Track edits, but also analyze diff context
4. **Tool Call Patterns**: Analyze tool selection as intent signal

### Layer 2: Intent Classification

```
Raw Activity → Feature Extraction → Intent Classifier → Intent Label
```

**Intent categories:**
- `refactoring`: Modifying existing code structure
- `implementing`: Adding new functionality
- `testing`: Writing or modifying tests
- `debugging`: Fixing bugs or issues
- `documenting`: Creating or updating docs
- `analyzing`: Researching or understanding code
- `configuring`: Setting up or modifying config
- `deploying`: Shipping or deploying code

### Layer 3: Intent Understanding

```
Intent Label + Context → Intent Understanding → Coordination Suggestion
```

**Understanding components:**
- **Scope**: What files/modules are affected?
- **Complexity**: How large is the change?
- **Dependencies**: What other components are involved?
- **Goal**: What is the agent trying to achieve?

---

## Implementation Design

### Data Structures

```rust
/// Represents an agent's current intent
struct AgentIntent {
    agent_id: String,
    agent_type: AgentType,  // claude, codex, opencode, minimax, gpt
    intent_category: IntentCategory,
    scope: Vec<PathBuf>,    // Files/modules
    complexity: Complexity, // low, medium, high
    goal: String,          // Natural language goal
    confidence: f32,        // 0.0 - 1.0
    timestamp: DateTime<Utc>,
    context: Vec<IntentContext>,
}

enum IntentCategory {
    Refactoring,
    Implementing,
    Testing,
    Debugging,
    Documenting,
    Analyzing,
    Configuring,
    Deploying,
    Unknown,
}

struct IntentContext {
    key: String,
    value: String,
}
```

### Intent Detection Pipeline

```
┌─────────────────┐
│  Agent Action   │
│  (hook/event)   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Feature        │
│  Extraction     │
└────────┬────────┘
         │
    ┌────┴────┐
    │ Local    │  ← Fast path: rule-based
    │ (simple) │
    └────┬────┘
         │
         ▼ (if needed)
┌─────────────────┐
│  AI-Powered    │
│  Classification│  ← LLM-based for complex cases
│  (async)       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Intent        │
│  Store         │
└─────────────────┘
```

### Provider Abstraction

```rust
trait IntentProvider: Send + Sync {
    /// Classify intent from activity
    fn classify(&self, activity: &Activity) -> impl Future<Output = Result<Intent, Error>>;
    
    /// Extract goal from agent output
    fn extract_goal(&self, output: &str) -> impl Future<Output = Result<String, Error>>;
    
    /// Check if provider is available
    fn is_available(&self) -> bool;
}

/// Built-in providers
enum IntentProviderImpl {
    RuleBased,      // Fast, no API calls
    Claude,         // Claude API
    OpenAI,         // OpenAI API  
    Minimax,       // Minimax API
    LocalModel,    // Local LLM (ollama, etc.)
}
```

---

## Coordination Integration

### Intent-Based Coordination

When multiple agents have detected intents:

```
Agent A: Intent { category: Refactoring, scope: [auth/], goal: "simplify token handling" }
Agent B: Intent { category: Implementing, scope: [auth/], goal: "add JWT support" }

→ Intent Detector identifies: CONFLICT potential
→ Coordination engine suggests: "A should refactor first, then B implements"
→ Context injection: Provide A's refactored structure to B
```

### Intent Storage

```rust
/// Store for agent intents (in-memory + persisted)
struct IntentStore {
    intents: HashMap<AgentId, Vec<AgentIntent>>,
    coordination_log: Vec<CoordinationEvent>,
}

impl IntentStore {
    fn detect_conflicts(&self, intents: &[AgentIntent]) -> Vec<Conflict>;
    fn suggest_coordination(&self, conflicts: &[Conflict]) -> Vec<Suggestion>;
    fn inject_context(&self, suggestion: &Suggestion) -> ContextInjection;
}
```

---

## Real-Time Requirements

### Latency Targets

| Operation | Target Latency | Acceptable |
|-----------|---------------|------------|
| Rule-based classification | <50ms | 100ms |
| LLM-based classification | <500ms | 2s |
| Intent storage query | <10ms | 50ms |
| Conflict detection | <100ms | 200ms |

### Event Processing

- **Streaming**: Process intents as they arrive
- **Batching**: Group low-priority classifications
- **Caching**: Cache similar classifications

---

## Configuration

```json
{
  "intent_detection": {
    "enabled": true,
    "default_provider": "rule-based",
    "fallback_providers": ["claude", "openai"],
    "cache_ttl_seconds": 300,
    "llm_timeout_ms": 2000,
    "confidence_threshold": 0.7
  },
  "providers": {
    "claude": {
      "api_key_env": "ANTHROPIC_API_KEY",
      "model": "claude-3-sonnet"
    },
    "openai": {
      "api_key_env": "OPENAI_API_KEY",
      "model": "gpt-4"
    },
    "minimax": {
      "api_key_env": "MINIMAX_API_KEY",
      "model": "abab6.5s-chat"
    }
  }
}
```

---

## CLI Commands

```bash
# Intent detection
impulse intent status              # Show intent detection status
impulse intent agents             # Show current agent intents
impulse intent conflicts          # Show detected conflicts
impulse intent suggest <agent>    # Get coordination suggestions

# Provider management
impulse intent provider list      # List available providers
impulse intent provider set <name> # Set default provider
impulse intent provider test      # Test provider connectivity
```

---

## Roadmap

### Phase 1: Foundation (Loops 1-10)
- [ ] Define intent data structures
- [ ] Implement rule-based classifier
- [ ] Add intent storage
- [ ] Basic CLI commands

### Phase 2: AI Integration (Loops 11-20)
- [ ] Add Claude provider
- [ ] Add OpenAI provider
- [ ] Add Minimax provider
- [ ] Implement provider abstraction

### Phase 3: Coordination (Loops 21-30)
- [ ] Conflict detection algorithm
- [ ] Coordination suggestions
- [ ] Context injection integration

### Phase 4: Refinement (Loops 31-40)
- [ ] Performance optimization
- [ ] Caching strategies
- [ ] Error handling
- [ ] Testing and verification

---

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Intent classification accuracy | >85% | Manual review of samples |
| False positive rate | <10% | Conflict detection review |
| Classification latency (LLM) | <500ms | P95 latency |
| Classification latency (rule) | <50ms | P95 latency |
| Coverage | >90% of agent actions | Hook event coverage |

---

## Open Questions

1. **How to handle intent drift?** Agent goals may change mid-session
2. **How much context to include?** vs. latency trade-off
3. **Provider selection strategy?** Cost vs. accuracy vs. speed
4. **Privacy implications?** Storing agent goals

---

## Related Documents

- [RUST-CANONICAL-CONTRACT.md](../spec/RUST-CANONICAL-CONTRACT.md)
- [DYNAMIC-CLI-VISION.md](./DYNAMIC-CLI-VISION.md)
- [REAL-TIME-INJECTION-VISION.md](./REAL-TIME-INJECTION-VISION.md)

---

_Created: 2026-02-25_
_Ralph Loop: Intent Detection Vision_
