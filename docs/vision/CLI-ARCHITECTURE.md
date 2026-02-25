---
status: active
phase: 2-3
audience: builder
tags: [vision, cli, architecture]
last_updated: 2026-02-20
---

# CLI++ Architecture: Interactive Command System

> **Version:** 1.0 | **Status:** Design | **Updated:** 2026-02-20
> **Scope:** Command structure, implementation strategy, Zellij integration

---

## Overview

CLI++ is a **two-tier command system**:

1. **Zellij-native** (`:swarm` commands in Zellij pane)
2. **Standalone** (`swarm` CLI for terminal use)

Both interact with the same **SWARM daemon** via IPC/REST.

---

## Architecture Layers

```
┌─────────────────────────────────────────────────────────┐
│                    Developer (Human)                     │
├─────────────────────────────────────────────────────────┤
│            CLI++ Interface Layer                         │
│  ┌──────────────────────┐  ┌──────────────────────┐     │
│  │  Zellij Commands     │  │  Standalone CLI      │     │
│  │  :swarm patterns     │  │  swarm patterns      │     │
│  │  :swarm timeline     │  │  swarm timeline      │     │
│  │  :swarm decisions    │  │  swarm decisions     │     │
│  └──────────┬───────────┘  └──────────┬───────────┘     │
├─────────────┼──────────────────────────┼─────────────────┤
│         Communication Layer (IPC/REST)                   │
│  Unix socket (local) or REST API (remote)               │
├─────────────┼──────────────────────────┼─────────────────┤
│            SWARM Daemon (Core)                           │
│  ┌──────────────────────┐  ┌──────────────────────┐     │
│  │  Command Router      │  │  State Manager       │     │
│  │  Pattern Engine      │  │  Decision Logger     │     │
│  │  Pattern Detector    │  │  Learning System     │     │
│  └──────────┬───────────┘  └──────────┬───────────┘     │
├─────────────┼──────────────────────────┼─────────────────┤
│               Data Layer                                 │
│  ┌──────────────────────┐  ┌──────────────────────┐     │
│  │  live_state.db       │  │  decisions.json      │     │
│  │  (vectors, patterns) │  │  (coordination log)  │     │
│  │  (sqlite-vec)        │  │                      │     │
│  └──────────┬───────────┘  └──────────┬───────────┘     │
└─────────────┴──────────────────────────┴─────────────────┘
```

---

## Zellij Command System

### Architecture

Zellij commands run as **event handlers** in the SWARM daemon:

```typescript
// Command registration
const zellij = new ZellijCommandHandler({
  prefix: 'swarm',
  daemon: swarmDaemon,
});

zellij.register('patterns', async (args, output) => {
  const patterns = swarmDaemon.getPatterns(args);
  output.render(formatPatternsTable(patterns));
});

zellij.register('timeline', async (args, output) => {
  const events = swarmDaemon.getTimeline(args);
  output.stream(formatTimelineStream(events));
});
```

### Command Categories

**Discovery Commands:**
```bash
:swarm agents              # List active agents
:swarm agents --detail     # Detailed agent info
:swarm files               # Watched files
:swarm patterns            # All patterns
:swarm decisions           # Major decisions
```

**Monitoring Commands:**
```bash
:swarm watch patterns      # Live pattern stream
:swarm watch timeline      # Live event stream
:swarm watch health        # Health metrics
:swarm metrics             # Detailed metrics
```

**Steering Commands:**
```bash
:swarm suggest <a1> <a2>   # Suggest coordination
:swarm inject <agent> <msg> # Manual injection
:swarm pause <agent>       # Pause injections
:swarm resume <agent>      # Resume injections
:swarm reset               # Clear patterns
```

**Learning Commands:**
```bash
:swarm learn <pattern-id>  # Extract rule
:swarm rules               # List learned rules
:swarm feedback <id> good  # Training signal
:swarm export rules        # Export for next session
```

**Analysis Commands:**
```bash
:swarm analyze --since 1h  # Time-based analysis
:swarm diff <a1> <a2>      # Compare agents
:swarm impact <pattern-id> # Pattern impact
:swarm report              # Session summary
```

---

## Standalone CLI

### Implementation

```typescript
// swarm-cli.ts
import { SwarmDaemon } from './daemon';
import { Command } from 'commander';

const program = new Command()
  .name('swarm')
  .description('Multi-agent coordination CLI');

// Patterns command
program
  .command('patterns')
  .option('--file <path>', 'Filter by file')
  .option('--agent <id>', 'Filter by agent')
  .option('--json', 'JSON output')
  .action(async (options) => {
    const daemon = new SwarmDaemon();
    const patterns = daemon.getPatterns(options);

    if (options.json) {
      console.log(JSON.stringify(patterns, null, 2));
    } else {
      console.log(formatPatternsTable(patterns));
    }
  });

// Learn command
program
  .command('learn <patternId>')
  .description('Extract coordination rule from pattern')
  .action(async (patternId) => {
    const daemon = new SwarmDaemon();
    const rule = await daemon.learnRule(patternId);
    console.log(`✓ Learned rule: ${rule.description}`);
    console.log(`  Confidence: ${rule.confidence}`);
  });
```

### Usage Examples

```bash
# View patterns affecting current file
swarm patterns --file src/auth.ts

# Export decisions for documentation
swarm decisions --format markdown > COORDINATION.md

# Analyze agent collaboration
swarm analyze --since 2h --format json | jq '.patterns[] | select(.confidence > 0.85)'

# Train system on successful coordination
swarm feedback pattern-123 good

# Export learned rules for next session
swarm export rules > coordination-rules.json
```

---

## Communication Protocol

### Daemon Socket Interface

```typescript
// IPC via Unix socket
interface DaemonMessage {
  id: string;
  command: string;
  args: Record<string, unknown>;
  timestamp: number;
}

interface DaemonResponse {
  id: string;
  status: 'success' | 'error';
  data?: unknown;
  error?: string;
  timestamp: number;
}
```

### REST API (Optional, for Remote Access)

```
GET /api/patterns
  ?file=src/auth.ts
  &confidence_min=0.85
  &since=1h

POST /api/decisions/feedback
  { patternId, feedback: 'good' | 'bad' }

GET /api/timeline
  ?since=10m
  &format=json

GET /api/metrics
  (returns latency, memory, coordination health)
```

---

## Output Formatting

### Pattern Table Format

```
PATTERNS (Last 10 minutes, confidence > 0.80)
─────────────────────────────────────────────────────────
ID         │ Topic              │ Agents        │ Conf  │ Status
───────────┼────────────────────┼───────────────┼───────┼────────
pat-001    │ auth refactor      │ Claude, Code  │ 0.92  │ ✓ Injected
pat-002    │ error handling     │ OpenCode      │ 0.87  │ ✓ Resolved
pat-003    │ test coverage      │ Aider, Claude │ 0.78  │ ◇ Monitoring
pat-004    │ database schema    │ Code, Aider   │ 0.71  │ ◇ Monitoring
─────────────────────────────────────────────────────────
Total: 4 patterns | Success rate: 75% | Avg confidence: 0.82
```

### Timeline Stream Format

```
LIVE TIMELINE (Real-time events)
─────────────────────────────────────────────────────────

12:47:03 │ Claude-Code  │ editing     │ src/auth.ts
12:47:08 │ OpenCode     │ editing     │ src/auth.ts ← MATCH (0.92)
12:47:15 │ SWARM        │ pattern     │ auth_refactor
12:47:22 │ Claude-Code  │ acknowledged│ [SWARM] injection
12:47:31 │ Aider        │ editing     │ src/auth.test.ts
12:47:45 │ SWARM        │ decision    │ auth_module_split
─────────────────────────────────────────────────────────
```

### Decision Log Format

```
DECISIONS (Major coordination milestones)
─────────────────────────────────────────────────────────

12:35:00 │ AUTH_MODULE_SPLIT
         │ Agents: Claude-Code (token validation)
         │         OpenCode (session refresh)
         │ Confidence: 0.94 │ Status: ✓ Success
         │ Outcome: 180 lines duplication removed

12:42:15 │ TEST_GENERATION_PAIR
         │ Agents: Aider (generation)
         │         Claude-Code (review)
         │ Confidence: 0.87 │ Status: ✓ Success
         │ Outcome: 15 new tests in 8 minutes

12:51:30 │ ERROR_HANDLING_STANDARDIZATION
         │ Agents: OpenCode (pattern detection)
         │         All (adoption)
         │ Confidence: 0.89 │ Status: ✓ Success
         │ Outcome: 12 existing errors refactored
─────────────────────────────────────────────────────────
```

---

## Error Handling

### Command Errors

```typescript
// Graceful error responses
:swarm patterns --agent invalid-id
// Error: Agent not found: invalid-id
// Hint: Use :swarm agents to see available agents

swarm timeline --since 25h
// Error: Time range invalid: 25h > session duration (12h)
// Hint: Use --since 12h or less

:swarm learn pattern-999
// Error: Pattern not found: pattern-999
// Hint: Use :swarm patterns to see available patterns
```

### Daemon Connection Errors

```typescript
// If daemon is down
swarm patterns
// Error: Cannot connect to SWARM daemon
// Suggestion: Is the daemon running?
//   Start with: swarm daemon start
//   Or restart Zellij workspace

// Automatic reconnection with backoff
// Retry 1 (100ms) → Retry 2 (200ms) → Retry 3 (400ms) → Give up
```

---

## Performance Targets

| Command | Target | Notes |
|---------|--------|-------|
| `patterns` | <100ms | In-memory lookup |
| `timeline` | <200ms | DB query + format |
| `decisions` | <150ms | Filtered log read |
| `learn` | <500ms | LLM call for rule extraction |
| `export` | <1s | File write |
| Stream commands | <50ms latency | Real-time updates |

---

## Future Extensions

### 1. Scripting Interface

```bash
# swarm-script.sh
swarm patterns --json | \
  jq '.[] | select(.confidence > 0.9)' | \
  while read pattern; do
    swarm feedback "$(echo $pattern | jq -r .id)" good
  done
```

### 2. API Gateway

```bash
# Access SWARM from other tools
curl http://localhost:3333/api/patterns?confidence_min=0.85
curl http://localhost:3333/api/timeline?since=5m --stream
```

### 3. Web Dashboard (Phase 3+)

```typescript
// Companion web UI for complex visualization
GET http://localhost:3334/dashboard
// Shows: Pattern graph, decision timeline, agent collaboration
```

---

## Implementation Order

### Phase 1:
- [ ] Command router (basic)
- [ ] Zellij command bindings (`:swarm patterns`, etc.)
- [ ] Simple output formatting

### Phase 1.5:
- [ ] All discovery/monitoring commands
- [ ] Stream commands (watch)
- [ ] Learning commands (learn, feedback)

### Phase 2:
- [ ] Standalone CLI tool
- [ ] JSON output modes
- [ ] REST API

### Phase 3+:
- [ ] Advanced scripting
- [ ] Web dashboard
- [ ] Multi-workspace CLI

---

## References

- Zellij plugin system: cloned-repos/zellij/
- Commander.js (CLI framework): https://github.com/tj/commander.js
- DYNAMIC-CLI-VISION.md (User-facing workflows)

---

_Created: 2026-02-20 | Status: Design v1.0 | Ready for Implementation_
