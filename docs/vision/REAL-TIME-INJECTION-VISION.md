---
title: Real-Time Injection Vision
description: Real-time context injection for Impulse agent orchestration
version: '1.0'
updated: 2026-02-25
type: vision
category: core
phase: 3
status: active
audience: builders
tags: [vision, real-time, injection, context, orchestration]
authors:
  - name: Impulse Maintainers
    role: Maintainer
    email: impulse-rs@users.noreply.github.com
    github: Jimthetaxguy/IMPULSE-rs
---

# Real-Time Injection Vision

> **Vision:** Context injection into Impulse agents happens in real-time as agents work, enabling proactive coordination and context-aware assistance without disrupting agent workflow.

---

## Current State

The current Impulse architecture supports:
- **Review-first injection**: Context staged as artifacts, reviewed before injection
- **Hook-based triggers**: `on_tool_call`, `on_result`, etc.
- **Daemon mode**: Background daemon for chat with context
- **Orchestration handoffs**: Explicit handoff context files

**What's missing:** True real-time injection without review step for seamless coordination.

---

## Target State

### Core Concept: Real-Time Injection

Real-time injection means context is delivered to agents **as they work**, not after the fact:

- **Immediate**: Injection happens within 100-500ms of detection
- **Non-blocking**: Agent continues working while context is prepared
- **Smart**: Context is filtered and relevance-scored before injection
- **Transparent**: Agent knows source and reason for context

### Injection Triggers

| Trigger | Latency Target | Example |
|---------|---------------|---------|
| Pattern detected | <200ms | Two agents editing same file |
| Intent conflict | <300ms | Agents with conflicting goals |
| Context opportunity | <500ms | Agent working on file with relevant history |
| Explicit request | <100ms | Agent requests context |

### Injection Modes

```
┌─────────────────────────────────────────────────────────────┐
│                    INJECTION MODES                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  OFF          → No automatic injection                      │
│                Manual only via CLI                         │
│                                                             │
│  REVIEW        → Context staged as artifact                 │
│                Agent reviews before accepting              │
│                (Current default)                           │
│                                                             │
│  APPLY         → Context injected immediately              │
│                Agent receives and integrates               │
│                (Real-time mode)                            │
│                                                             │
│  HYBRID        → Critical: apply immediately               │
│                Non-critical: review first                  │
│                (Smart mode)                                │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## Architecture Design

### Injection Pipeline

```
┌─────────────────┐
│  Trigger       │  ← Pattern, Intent, Request
│  Detection     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Context       │  ← Query retrieval, Intent store, Genome
│  Gathering     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Relevance     │  ← Score and filter context
│  Scoring       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Formatting    │  ← Format for agent type
│  (async)       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Delivery      │  ← PTY, MCP, API
│  (async)       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Confirmation  │  ← Agent acknowledges
│  (async)       │
└─────────────────┘
```

### Components

```rust
/// Core injection engine
struct InjectionEngine {
    trigger_detector: TriggerDetector,
    context_gatherer: ContextGatherer,
    relevance_scorer: RelevanceScorer,
    context_formatter: ContextFormatter,
    delivery_service: DeliveryService,
}

/// Detects events that should trigger injection
trait TriggerDetector: Send + Sync {
    fn detect(&self, event: &Event) -> Vec<Trigger>;
}

/// Gathers relevant context for a trigger
trait ContextGatherer: Send + Sync {
    fn gather(&self, trigger: &Trigger) -> impl Future<Output = Result<Vec<Context>, Error>>;
}

/// Scores context relevance
trait RelevanceScorer: Send + Sync {
    fn score(&self, context: &Context, trigger: &Trigger) -> f32;
}

/// Formats context for specific agent type
trait ContextFormatter: Send + Sync {
    fn format(&self, context: &Context, agent: &AgentType) -> String;
}

/// Delivers formatted context to agent
trait DeliveryService: Send + Sync {
    fn deliver(&self, formatted: &str, agent: &AgentId) -> impl Future<Output = Result<DeliveryStatus, Error>>;
}
```

---

## Context Sources

### Primary Sources

1. **Project Genome**: Decisions, preferences, patterns
2. **Session History**: What was done in this session
3. **Project History**: Past sessions on this project
4. **Intent Store**: Current agent intents (from intent detection)
5. **Coordination Log**: Past coordination events

### Context Types

| Type | Description | Priority |
|------|-------------|----------|
| `coordination` | Agent coordination suggestions | High |
| `history` | Relevant past work | Medium |
| `genome` | Project decisions/preferences | Medium |
| `tool_context` | Tool usage patterns | Low |
| `dependency` | Dependency information | Medium |

---

## Agent-Specific Delivery

### Claude Code (MCP)

```rust
// MCP tool call injection
struct ClaudeDelivery {
    mcp_connection: McpConnection,
}

impl DeliveryService for ClaudeDelivery {
    async fn deliver(&self, context: &str, agent: &AgentId) -> Result<DeliveryStatus, Error> {
        // Use MCP progress notification or custom tool
        self.mcp_connection
            .notify("context/inject", json!({ "content": context }))
            .await
    }
}
```

### Codex / OpenCode (Hook Events)

```rust
// Hook event delivery
struct HookDelivery {
    hook_socket: UnixSocket,
}

impl DeliveryService for HookDelivery {
    async fn deliver(&self, context: &str, agent: &AgentId) -> Result<DeliveryStatus, Error> {
        self.hook_socket
            .send(HookEvent::ContextInjection {
                content: context.to_string(),
                source: "impulse".to_string(),
                timestamp: Utc::now(),
            })
            .await
    }
}
```

### Shell / Direct PTY

```rust
// Direct PTY input injection
struct PtyDelivery {
    pty_master: PtyMaster,
}

impl DeliveryService for PtyDelivery {
    async fn deliver(&self, context: &str, agent: &AgentId) -> Result<DeliveryStatus, Error> {
        // Write to PTY with marker for detection
        let marked = format!("\n[Impulse Context]\n{}\n[/Impulse Context]\n", context);
        self.pty_master.write(marked.as_bytes())?;
        Ok(DeliveryStatus::Delivered)
    }
}
```

---

## Real-Time Processing

### Event Stream Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    EVENT PROCESSING                           │
└──────────────────────────────────────────────────────────────┘

  File Watcher ──► Activity Stream ──► Intent Detection
        │                │                    │
        │                │                    ▼
        │                │              Intent Store
        │                │                    │
        ▼                ▼                    ▼
  Trigger           Trigger             Trigger
  Detector          Detector            Detector
        │                │                    │
        └────────────────┼────────────────────┘
                         │
                         ▼
              ┌─────────────────────┐
              │   Injection Queue   │  ← Priority queue
              │   (async workers)   │
              └──────────┬──────────┘
                         │
         ┌───────────────┼───────────────┐
         │               │               │
         ▼               ▼               ▼
    Context         Context         Context
    Gatherer        Gatherer        Gatherer
         │               │               │
         ▼               ▼               ▼
    Relevance       Relevance       Relevance
    Scorer          Scorer          Scorer
         │               │               │
         └───────────────┼───────────────┘
                         │
                         ▼
              ┌─────────────────────┐
              │   Delivery Service   │
              └─────────────────────┘
```

### Worker Pool

```rust
struct InjectionWorkers {
    pool: Spawner<InjectionTask>,  // tokio task spawner
    max_concurrent: usize,        // Configurable
    queue: mpsc::Receiver<InjectionTask>,
}

impl InjectionWorkers {
    fn new(max_concurrent: usize) -> Self;
    
    async fn submit(&self, task: InjectionTask) -> Result<(), Error>;
    
    async fn process(&self) {
        while let Some(task) = self.queue.recv().await {
            // Process with timeout
            match tokio::time::timeout(Duration::from_secs(5), self.process_one(task)).await {
                Ok(Ok(status)) => { /* track metrics */ }
                Ok(Err(e)) => { /* log error */ }
                Err(_) => { /* timeout */ }
            }
        }
    }
}
```

---

## Rate Limiting & Safety

### Rate Limiting

```rust
struct RateLimiter {
    max_per_minute: usize,
    max_per_hour: usize,
    burst_allowance: usize,
    per_agent_limits: HashMap<AgentId, AgentLimit>,
}

impl RateLimiter {
    fn check(&mut self, agent: &AgentId) -> Result<(), RateLimitError>;
    
    fn adaptive_limit(&self, agent: &AgentId, success_rate: f32) -> usize {
        // Adjust limits based on acceptance rate
        let base = self.max_per_minute;
        if success_rate > 0.9 {
            base * 2  // Increase if agent accepts most
        } else if success_rate < 0.5 {
            base / 2  // Decrease if agent ignores most
        }
        base
    }
}
```

### Anti-Echo Protection

```rust
struct EchoProtector {
    recent_injections: RingBuffer<Injection>,
    min_time_between: Duration,
    max_per_window: usize,
}

impl EchoProtector {
    fn check(&self, injection: &Injection) -> Result<(), EchoError> {
        // Check if similar injection was recent
        for recent in self.recent_injections.iter() {
            if recent.target_agent == injection.target_agent
                && recent.content.similarity(&injection.content) > 0.9
                && recent.timestamp.elapsed() < self.min_time_between
            {
                return Err(EchoError::TooSimilar);
            }
        }
        Ok(())
    }
}
```

---

## Configuration

```json
{
  "injection": {
    "enabled": true,
    "default_mode": "review",  // off, review, apply, hybrid
    
    "rate_limits": {
      "max_per_minute": 5,
      "max_per_hour": 30,
      "burst_allowance": 2
    },
    
    "safety": {
      "echo_protection": true,
      "min_time_between_seconds": 30,
      "max_similarity": 0.9
    },
    
    "delivery": {
      "timeout_ms": 5000,
      "retry_attempts": 3,
      "retry_delay_ms": 1000
    },
    
    "context": {
      "max_tokens": 2000,
      "max_items": 5,
      "min_relevance_score": 0.6
    }
  },
  
  "modes": {
    "apply": {
      "auto_approve": true,
      "notify_after": true
    },
    "hybrid": {
      "critical_triggers": ["conflict", "dependency"],
      "non_critical_triggers": ["history", "tool_context"]
    }
  }
}
```

---

## Monitoring & Metrics

### Metrics to Track

| Metric | Type | Target |
|--------|------|--------|
| Injection latency (P95) | Latency | <500ms |
| Injection latency (P99) | Latency | <2s |
| Acceptance rate | Ratio | >70% |
| Rejection rate | Ratio | <20% |
| Echo rate | Ratio | <1% |
| Queue depth | Gauge | <10 |
| Worker utilization | Ratio | <80% |

### Logging

```rust
// Structured log for injection events
log::info!(
    target: "injection",
    agent = %agent_id,
    trigger = %trigger_type,
    latency_ms = latency.as_millis(),
    context_tokens = token_count,
    status = %status,
    "Injection completed"
);
```

---

## CLI Commands

```bash
# Injection management
impulse injection status           # Show injection status
impulse injection mode <mode>      # Set mode (off/review/apply/hybrid)
impulse injection test             # Test injection delivery

# Monitoring
impulse injection history          # Show recent injections
impulse injection metrics          # Show metrics
impulse injection health          # Health check

# Debugging
impulse injection dry-run <trigger> # Simulate injection
impulse injection log              # View injection logs
```

---

## Roadmap

### Phase 1: Foundation (Loops 1-10)
- [ ] Define injection data structures
- [ ] Implement trigger detection
- [ ] Build context gathering pipeline
- [ ] Basic delivery mechanisms

### Phase 2: Real-Time (Loops 11-20)
- [ ] Async injection workers
- [ ] Rate limiting
- [ ] Echo protection
- [ ] Priority queue

### Phase 3: Intelligence (Loops 21-30)
- [ ] Relevance scoring improvements
- [ ] Context formatting per agent
- [ ] Adaptive rate limiting
- [ ] Metrics and monitoring

### Phase 4: Polish (Loops 31-40)
- [ ] Error handling
- [ ] Retry logic
- [ ] Performance tuning
- [ ] Testing and verification

---

## Integration with Intent Detection

Real-time injection works hand-in-hand with intent detection:

```
Intent Detection                    Real-Time Injection
─────────────────                   ───────────────────
                                   
  Agent A: Intent                  ──► Trigger: Intent Conflict
    category: refactoring              Gathering: Context from 
    scope: [auth/]                       - Agent B's intent
    goal: "simplify                      - Genome decisions
                       tokens"           - Past refactors
                                          │
                                          ▼
                                    Relevance: Score 0.92
                                          │
                                          ▼
                                    Format: Coordination suggestion
                                          │
                                          ▼
                                    Deliver: To Agent A's PTY
                                          │
                                          ▼
                                    Confirm: Agent A acknowledges
```

---

## Related Documents

- [RUST-CANONICAL-CONTRACT.md](../spec/RUST-CANONICAL-CONTRACT.md)
- [DYNAMIC-CLI-VISION.md](./DYNAMIC-CLI-VISION.md)
- [INTENT-DETECTION-VISION.md](./INTENT-DETECTION-VISION.md)

---

_Created: 2026-02-25_
_Ralph Loop: Real-Time Injection Vision_
