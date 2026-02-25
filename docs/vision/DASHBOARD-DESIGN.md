---
status: active
phase: 3
audience: builder
tags: [vision, dashboard, ui]
last_updated: 2026-02-20
---

# Dashboard Design: Interactive Zellij UI

> **Version:** 1.0 | **Status:** Visual Design | **Updated:** 2026-02-20
> **Implementation:** Rust/WASM (Zellij plugin) | **Interactivity:** Keyboard-driven

---

## Core Dashboard Layout

```
┌────────────────────────────────────────────────────────────────┐
│  [Agent1: Claude] │ [Agent2: Code] │ [Agent3: Aider] │ SWARM  │
├──────────────────────────────────────────────────────────────┬─┤
│ COORDINATION HUB                                            │ │
│                                                             │ │
│ ACTIVE PATTERN: auth module refactor                       │ │
│ Confidence: ████████░░ 0.92 | Status: INJECTED            │S│
│                                                             │C│
│ Agents: Claude-Code (token validation)                    │R│
│         OpenCode (session refresh)                         │O│
│         Aider (test coverage)                              │L│
│                                                             │L│
│ Files involved: src/auth.ts, src/auth.test.ts              │ │
│                                                             │ │
│ Timeline: Detected 3m ago | Echo loops: 0                  │ │
│ Suggestion: Split token validation from session refresh    │ │
│                                                             │ │
│ Actions: [Accept] [Review] [Learn] [Ignore] [Details]      │ │
│                                                             │ │
├─────────────────────────────────────────────────────────────┤─┤
│ TIMELINE VIEW (Scrollable, color-coded)                    │ │
│                                                             │ │
│ 12:47:03 ◆ Claude-Code   editing src/auth.ts              │↓│
│ 12:47:08 ◆ OpenCode      editing src/auth.ts             │ │
│          ↳ 🔗 PATTERN (0.92) auth_refactor               │ │
│ 12:47:15 ✓ SWARM         detected + queued injection     │ │
│ 12:47:22 ← Claude-Code   acknowledged [SWARM] injection  │ │
│ 12:47:31 ◆ Aider         editing src/auth.test.ts        │ │
│          ↳ 🔗 PATTERN (0.85) auth scope (injected)       │↑│
│ 12:47:45 ✓ SWARM         recorded decision: auth_split   │ │
│                                                             │ │
├─────────────────────────────────────────────────────────────┤─┤
│ METRICS BAR                                                 │ │
│ Active: 3 agents | Patterns: 8 detected | Decisions: 3    │ │
│ Echo loops prevented: 2 | Avg confidence: 0.89             │ │
│ Context usage: ████░░░░░░ 45% | Session uptime: 12m       │ │
└────────────────────────────────────────────────────────────┴─┘
```

---

## Section 1: Coordination Hub (Top)

### Design

- **Always shows:** Current most important pattern
- **Interaction:** Tab through active patterns (if >1)
- **Updates:** Real-time when pattern confidence changes

### Visual State Indicators

```
ACTIVE PATTERN: auth module refactor              [Status Badges]
Confidence: ████████░░ 0.92                      │ ◇ MONITORING
  └─ Similarity: 0.92                            │ ✓ INJECTED
  └─ Decay: -0.08 (8 min old)                    │ ❌ BLOCKED
  └─ Freshness: +0.00                            │ ⧖ QUEUED
```

### Action Buttons

```
[Accept]     - Acknowledge coordination, continue
[Review]     - See detailed explanation
[Learn]      - Extract as coordination rule
[Ignore]     - Dismiss (don't show similar)
[Details]    - Full pattern analysis
[Override]   - Manual adjustment (for expert users)
```

### Keyboard Shortcuts

```
Tab              → Next pattern
Shift+Tab        → Previous pattern
A                → Accept
R                → Review details
L                → Learn rule
I                → Ignore
D                → Show details
:                → Command mode (Zellij-style)
```

---

## Section 2: Timeline View (Middle)

### Events with Color Coding

```
🟢 Event start (agent begins work)
🔗 Pattern detection (similar work found)
✓ Successful action (injection sent, rule learned)
❌ Error or blocked event
⧖ Queued (waiting for opportunity)
⟳ Decision made (major milestone)
📊 Metric milestone
```

### Interactive Timeline

```
12:47:22 ✓ SWARM         injected suggestion to Claude-Code
         ┗━ [h] help  [d] details  [e] explain  [u] undo

         Shows:
         - What was injected (120 token limit)
         - Why (pattern similarity, confidence)
         - Whether it was used
```

### Scrolling & Navigation

```
↑/↓              → Scroll timeline
G / Shift+G      → Go to start / end
PageUp/PageDown  → Page scroll
/pattern         → Search timeline
n / N            → Next / previous match
```

---

## Section 3: Metrics Bar (Bottom)

### Real-Time Metrics

```
┌─ Agent Status ────────────────────────────────────────────┐
│ Active: 3/6  │ Idle: 2  │ Error: 1                       │
└──────────────────────────────────────────────────────────┘

┌─ Coordination ────────────────────────────────────────────┐
│ Patterns detected: 8  │ Active: 1  │ Resolved: 3          │
│ Decisions made: 3     │ Success rate: 87%                 │
│ Echo loops prevented: 2                                    │
└──────────────────────────────────────────────────────────┘

┌─ Performance ─────────────────────────────────────────────┐
│ Avg latency: 120ms │ Memory: 18.2 MB │ CPU: 2.3%         │
│ Pattern detection: 34 patterns/hour                        │
└──────────────────────────────────────────────────────────┘

┌─ Context Usage ───────────────────────────────────────────┐
│ Current: ████░░░░░░ 45% | Peak: 87% | Budget: 32K tokens │
└──────────────────────────────────────────────────────────┘
```

### Alert Indicators

```
🟢 All good (confidence >0.85, no echoes)
🟡 Caution (confidence 0.70-0.85, few echoes)
🔴 Alert (echo loops, rate limits hit, errors)
⚫ Critical (daemon unhealthy, data loss risk)
```

---

## View Modes

### Mode 1: Coordination Focus (Default)

```
Maximized: Coordination Hub + Timeline
Minimized: Metrics bar (always visible)
Keyboard: Focus on Hub actions (A/R/L)
```

### Mode 2: Timeline Analysis

```
Minimized: Coordination Hub (1 line summary)
Maximized: Full timeline with details
Keyboard: Focus on timeline navigation (/search, n/N)
```

### Mode 3: Metrics Dashboard

```
Minimized: Hub and Timeline (summary only)
Maximized: Detailed metrics with charts
Keyboard: Drill-down into specific metrics
```

### Mode 4: Commands

```
Enter command mode: ':'
Shows: Autocomplete list for :swarm commands
Examples:
  :swarm patterns
  :swarm learn <id>
  :swarm feedback <id> good
  :swarm analyze --since 1h
```

**Navigation:**
```
Ctrl+1           → Coordination mode
Ctrl+2           → Timeline mode
Ctrl+3           → Metrics mode
Ctrl+4           → Command mode
Esc              → Exit command mode
```

---

## Detailed Pattern View

When user presses 'R' (Review):

```
┌────────────────────────────────────────────────────────┐
│ PATTERN ANALYSIS: Auth Module Refactor                 │
├────────────────────────────────────────────────────────┤
│                                                         │
│ What SWARM Detected:                                   │
│ ─────────────────────────────                          │
│ Two agents working on auth module simultaneously       │
│ - Claude-Code: Focusing on token validation logic     │
│ - OpenCode: Focusing on session management            │
│                                                         │
│ Why This Matters:                                      │
│ ─────────────────────────────                          │
│ These are related but distinct concerns. Without       │
│ coordination, you'll likely:                           │
│ • Duplicate error handling logic                       │
│ • Create conflicting token refresh strategies          │
│ • Miss test coverage for edge cases                    │
│                                                         │
│ Suggestion:                                            │
│ ─────────────────────────────                          │
│ Split responsibilities:                                │
│ • Claude-Code: Handle token validation + refresh       │
│ • OpenCode: Handle session lifecycle                   │
│ • Aider: Add tests for integration points              │
│                                                         │
│ Confidence: 0.92                                       │
│ • Similarity score: 0.92 (very high)                  │
│ • Detected 3 min ago (fresh)                           │
│ • Learned rule: Applies to similar auth work           │
│                                                         │
│ Evidence:                                              │
│ ─────────────────────────────                          │
│ File overlap: src/auth.ts                              │
│ Time overlap: 5 minutes (12:47 - 12:52)                │
│ Token usage: Both editing same 50-line section         │
│                                                         │
│ [Accept] [Learn] [Ignore] [Back]                       │
│                                                         │
└────────────────────────────────────────────────────────┘
```

---

## Learn Mode

When user presses 'L' (Learn):

```
┌────────────────────────────────────────────────────────┐
│ EXTRACT COORDINATION RULE                               │
├────────────────────────────────────────────────────────┤
│                                                         │
│ Pattern: Auth Module Refactor                          │
│                                                         │
│ Rule Name: auth_responsibilities_split                 │
│ (Edit or press Enter to accept)                        │
│                                                         │
│ Description:                                           │
│ When agents are refactoring auth module:               │
│ • Split token validation from session management       │
│ • One agent handles token logic                        │
│ • One agent handles session logic                      │
│ • Third agent adds test coverage                       │
│                                                         │
│ Trigger Conditions:                                    │
│ ✓ File contains: "auth" or "token" or "session"       │
│ ✓ Multiple agents editing same file                    │
│ ✓ Work started within 5 minutes of each other          │
│                                                         │
│ Confidence: 0.92                                       │
│ Can be adjusted: [Increase] [Decrease]                 │
│                                                         │
│ Application Scope:                                     │
│ ✓ Apply to similar patterns going forward              │
│ ✓ Export rule for next session                         │
│                                                         │
│ [Create Rule] [Cancel] [Preview]                       │
│                                                         │
└────────────────────────────────────────────────────────┘
```

---

## Performance Considerations

### Rendering
- Dashboard: <50ms frame time (smooth scrolling)
- Timeline: Virtualized rendering (only visible rows)
- Metrics: Update every 1 second (not realtime)

### Memory
- Dashboard pane: <5MB (Zellij plugin)
- Timeline buffer: Last 1000 events (max 2MB)
- Pattern list: Cached in daemon (not in plugin)

### Responsiveness
- Keyboard shortcuts: <100ms latency
- View mode switching: <200ms
- Pattern drill-down: <300ms (network call to daemon)

---

## Accessibility

### Keyboard-First Design
- All actions available via keyboard
- No mouse required (Zellij-native)
- Clear focus indicators

### Color + Icons
- Not reliant on color alone
- Icons (✓, ◆, 🔗) for clarity
- Contrast ratios WCAG AA

### Text Alternatives
- Metric bars have text values
- Colors have text labels
- Help available with `?` key

---

## Implementation Roadmap

### Phase 1.5:
- [ ] Basic Coordination Hub (Zellij plugin, Rust)
- [ ] Timeline view (virtualized)
- [ ] Metrics bar (hardcoded)
- [ ] Keyboard navigation
- [ ] Accept/Review/Learn buttons

### Phase 2:
- [ ] Detailed pattern view
- [ ] Learn/Extract rule UI
- [ ] Command mode (`:swarm`)
- [ ] View mode switching
- [ ] Real-time metric updates

### Phase 3+:
- [ ] Metrics dashboard (detailed)
- [ ] Analytics & charts
- [ ] Multi-session dashboard
- [ ] Customizable layouts

---

## References

- Zellij plugin system: cloned-repos/zellij/
- Ratatui (TUI framework): https://github.com/ratatui/ratatui
- DYNAMIC-CLI-VISION.md (User workflows)
- CLI-ARCHITECTURE.md (Command system)

---

_Created: 2026-02-20 | Status: Design v1.0 | Ready for Rust Implementation_
