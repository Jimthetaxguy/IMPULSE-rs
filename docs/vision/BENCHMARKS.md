# Benchmarks: Multi-Agent Coordination Test Plan

> **Version:** 1.0
> **Framework:** Custom Bash + sqlite3 CLI + Node.js harness tests
> **Date:** 2026-02-20

---

## Success Metrics Overview

| Metric | Target | Why | Measurement Method |
|--------|--------|-----|-------------------|
| **Coordination latency** | ≤30s | Pattern must reach agent before context becomes stale | Time from emergence to injection |
| **Overlap detection precision** | ≥0.85 | Avoid noisy false positives that confuse agents | True positives / (TP + FP) on labeled dataset |
| **Echo loop detection** | 0 cascades >2 hops | Prevent runaway propagation in 6-agent scenario | Count re-injections >2x per pattern |
| **RAM overhead** | ≤25MB | Harness must not dominate system on resource-constrained machines | Peak RSS during 6-agent test |
| **Context stability** | Fewer compactions vs baseline | SWARM injections should reduce context thrashing | Compare compaction count with/without SWARM |

---

## Test 1: Coordination Latency

**Goal:** Verify that pattern detection fires and injection is sent within 30 seconds.

**Setup:**
```bash
# Start mock OpenCode session with 2 agents
agent_A_session_id="session-latency-test-A"
agent_B_session_id="session-latency-test-B"

# Pre-populate patterns for Agent A (make it "discoverable")
sqlite3 ~/.impulse/live_state.db <<EOF
INSERT INTO active_agents (agent_id, session_id, agent_type, last_heartbeat, created_at)
VALUES ('agent-A', '$agent_A_session_id', 'opencode', datetime('now', 'unixepoch'), datetime('now'));
EOF
```

**Execution:**
```bash
# 1. Agent A sends 8 turns (establishes pattern)
for i in {1..8}; do
  emit_opencode_event message.updated \
    --agent-id agent-A \
    --role user \
    --content "Turn $i: Exploring authentication handler in auth.ts"
done

# Record timestamp of last message
start_time=$(date +%s)

# 2. Agent B sends similar message (trigger pattern detection)
emit_opencode_event message.updated \
  --agent-id agent-B \
  --role user \
  --content "Turn 1: Looking into session management in auth.ts"

# 3. Wait for injection
injection_found=0
for i in {1..30}; do
  sleep 1
  if sqlite3 ~/.impulse/live_state.db "SELECT COUNT(*) FROM injection_log WHERE target_agent='agent-B';" | grep -q "1"; then
    end_time=$(date +%s)
    latency=$((end_time - start_time))
    echo "✓ Injection detected in ${latency}s"
    injection_found=1
    break
  fi
done

if [ $injection_found -eq 0 ]; then
  echo "✗ FAIL: No injection within 30s"
  exit 1
fi
```

**Acceptance:**
- [ ] Latency ≤30s in 10 consecutive runs
- [ ] Latency p50 <10s, p95 <20s
- [ ] No timeouts or race conditions

---

## Test 2: Overlap Detection Precision

**Goal:** Verify that detected patterns are truly overlapping (precision ≥0.85).

**Setup:** Labeled dataset of 50 conversation pairs (agent A + agent B)
- 30 pairs with genuine overlap (expect pattern detection)
- 20 pairs with no overlap (expect no pattern)

**Execution:**
```bash
# For each pair in dataset:
for pair_id in {1..50}; do
  # 1. Load agent A's conversation
  agent_a_turns=$(jq -r '.pairs['$pair_id'].agent_a_turns' dataset.json)

  # 2. Load agent B's conversation
  agent_b_turns=$(jq -r '.pairs['$pair_id'].agent_b_turns' dataset.json)

  # 3. Emit A's turns
  for turn in $agent_a_turns; do
    emit_opencode_event message.updated --agent-id agent-A --content "$turn"
  done

  # 4. Emit B's turns
  for turn in $agent_b_turns; do
    emit_opencode_event message.updated --agent-id agent-B --content "$turn"
  done

  # 5. Check for injection
  pattern_detected=$(sqlite3 ~/.impulse/live_state.db \
    "SELECT COUNT(*) FROM injection_log WHERE target_agent='agent-B' AND timestamp > $((start_time - 60));")

  # 6. Verify against ground truth
  expected=$(jq -r '.pairs['$pair_id'].expected_overlap' dataset.json)
  if [ $pattern_detected -gt 0 ] && [ "$expected" == "true" ]; then
    true_positive=$((true_positive + 1))
  elif [ $pattern_detected -eq 0 ] && [ "$expected" == "false" ]; then
    true_negative=$((true_negative + 1))
  elif [ $pattern_detected -gt 0 ] && [ "$expected" == "false" ]; then
    false_positive=$((false_positive + 1))
  fi
done

precision=$((true_positive / (true_positive + false_positive)))
echo "Precision: $precision"
```

**Acceptance:**
- [ ] Precision ≥0.85 (TP / TP+FP ≥ 0.85)
- [ ] Recall ≥0.80 (TP / TP+FN ≥ 0.80)
- [ ] F1 score ≥0.82

---

## Test 3: Echo Loop Detection

**Goal:** Verify that SWARM prevents pattern re-injection cycles (0 cascades >2 hops).

**Setup:** 6-agent test scenario
```
Agent A starts working → detects overlap with B → injects to B
Agent B receives injection → modifies output → Agent C detects similarity
Agent C → injects to D, D → E, E → F (potential cascade)
Expected: Injection stops after 2 hops (anti-echo filter + rate limiter prevent cascade)
```

**Execution:**
```bash
# 1. Prime 6 agents with related but distinct messages
for agent_id in {1..6}; do
  emit_opencode_event message.updated \
    --agent-id agent-$agent_id \
    --role user \
    --content "Agent $agent_id: Working on feature X in module_$agent_id.ts"
done

# 2. Have Agent 1 and 2 start overlapping work
emit_opencode_event message.updated \
  --agent-id agent-1 \
  --role user \
  --content "Found shared auth utility in shared/auth.ts"

emit_opencode_event message.updated \
  --agent-id agent-2 \
  --role user \
  --content "Exploring shared auth utilities"

# Wait for first injection
sleep 5

# 3. Capture injection chain
injections=$(sqlite3 ~/.impulse/live_state.db <<EOF
SELECT source_agent, target_agent, pattern_id, timestamp
FROM injection_log
ORDER BY timestamp DESC;
EOF
)

# 4. Calculate hop depth for each pattern
pattern_hop_depth=$(echo "$injections" | awk '{
  pattern = $3
  if (prev_pattern != pattern) {
    if (hop_count > 2) {
      print "Pattern " pattern " cascaded " hop_count " hops (FAIL)"
      failed = 1
    }
    hop_count = 1
    prev_pattern = pattern
  } else {
    hop_count++
  }
} END {
  if (failed) exit 1
}')

if [ $? -eq 0 ]; then
  echo "✓ No cascades >2 hops detected"
else
  echo "✗ FAIL: Cascade detected"
  exit 1
fi
```

**Acceptance:**
- [ ] 0 patterns re-injected >2 times in 1-hour 6-agent test
- [ ] All cascades stop due to anti-echo filter or rate limiter (not timeout)
- [ ] Audit log shows rejection reasons for blocked injections

---

## Test 4: RAM Overhead

**Goal:** Verify harness memory usage stays ≤25MB during 6-agent test.

**Setup:**
```bash
# Start monitoring process
while true; do
  ps aux | grep steward | grep -v grep | awk '{print $6}' >> /tmp/steward_rss.txt
  sleep 2
done &
monitor_pid=$!

# Start 6-agent test (simulated, 1 hour)
```

**Execution:**
```bash
# Run 6-agent coordination test for 1 hour
for minute in {1..60}; do
  # Emit messages from all 6 agents
  for agent_id in {1..6}; do
    emit_opencode_event message.updated \
      --agent-id agent-$agent_id \
      --role user \
      --content "Agent $agent_id iteration $minute"
  done

  # Occasionally emit tool results
  if [ $((minute % 10)) -eq 0 ]; then
    emit_opencode_event tool.execute.after \
      --agent-id agent-1 \
      --tool-name "code_search" \
      --result-summary "Found 23 matches"
  fi

  sleep 60
done

# Stop monitoring
kill $monitor_pid

# Analyze RSS
max_rss=$(sort -n /tmp/steward_rss.txt | tail -1)
avg_rss=$(awk '{sum+=$1} END {print sum/NR}' /tmp/steward_rss.txt)

echo "Max RSS: ${max_rss}KB, Avg RSS: ${avg_rss}KB"

if [ $max_rss -gt 25000 ]; then
  echo "✗ FAIL: Memory exceeded 25MB"
  exit 1
else
  echo "✓ Memory usage within limit"
fi
```

**Acceptance:**
- [ ] Peak RSS ≤25MB during 1-hour 6-agent test
- [ ] Average RSS ≤20MB
- [ ] No memory leaks detected (RSS stable after 30 min warm-up)

---

## Test 5: Context Stability

**Goal:** Verify that SWARM injections reduce compaction count vs baseline.

**Setup:** 2 parallel 1-hour sessions
- **Baseline:** SWARM disabled (config `injection.enabled = false`)
- **Active:** SWARM enabled (config `injection.enabled = true`)

**Execution:**
```bash
# Session A: SWARM disabled
export STEWARD_CONFIG="injection.enabled=false"
start_time_a=$(date +%s)
for minute in {1..60}; do
  # Simulate a 4-agent scenario with overlapping work
  for agent_id in {1..4}; do
    emit_opencode_event message.updated \
      --agent-id agent-a-$agent_id \
      --role user \
      --content "Agent $agent_id: Iteration $minute (working on overlapping tasks)"
  done

  # Emit compaction hooks from each agent
  for agent_id in {1..4}; do
    emit_opencode_event session.compacting \
      --agent-id agent-a-$agent_id \
      --context-usage-pct $((50 + RANDOM % 40))
  done

  sleep 60
done
end_time_a=$(date +%s)

compactions_baseline=$(sqlite3 ~/.impulse/live_state.db \
  "SELECT COUNT(*) FROM injection_log WHERE decision_reason LIKE 'compaction%' AND timestamp BETWEEN $start_time_a AND $end_time_a;")

# Session B: SWARM enabled
export STEWARD_CONFIG="injection.enabled=true"
start_time_b=$(date +%s)
for minute in {1..60}; do
  for agent_id in {1..4}; do
    emit_opencode_event message.updated \
      --agent-id agent-b-$agent_id \
      --role user \
      --content "Agent $agent_id: Iteration $minute (working on overlapping tasks)"
  done

  for agent_id in {1..4}; do
    emit_opencode_event session.compacting \
      --agent-id agent-b-$agent_id \
      --context-usage-pct $((50 + RANDOM % 40))
  done

  sleep 60
done
end_time_b=$(date +%s)

compactions_active=$(sqlite3 ~/.impulse/live_state.db \
  "SELECT COUNT(*) FROM injection_log WHERE decision_reason LIKE 'compaction%' AND timestamp BETWEEN $start_time_b AND $end_time_b;")

# Compare
reduction=$((compactions_baseline - compactions_active))
reduction_pct=$((reduction * 100 / compactions_baseline))

echo "Baseline compactions: $compactions_baseline"
echo "Active compactions: $compactions_active"
echo "Reduction: $reduction ($reduction_pct%)"

if [ $reduction_pct -gt 10 ]; then
  echo "✓ Context stability improved"
else
  echo "✗ FAIL: Expected >10% reduction"
  exit 1
fi
```

**Acceptance:**
- [ ] Active SWARM reduces compaction triggers by ≥10% vs baseline
- [ ] Agents with injections report higher confidence in decisions
- [ ] Time-to-task-completion reduced (qualitative)

---

## Integration Test: End-to-End Coordination

**Goal:** Full scenario test combining all mechanisms.

**Scenario:**
```
1. Agent A and Agent B start exploring authentication handler
2. SWARM detects overlap, injects to Agent B
3. Agent B acknowledges the shared approach
4. Agent C joins with similar work
5. Verify: No echo loops, all injections logged, patterns promoted after 10 min
```

**Run:**
```bash
bash tests/integration/e2e_coordination.sh
```

**Expected output:**
```
[1/5] Setup: 3 agents ✓
[2/5] Emit overlapping turns ✓
[3/5] Verify pattern detection ✓
[4/5] Check anti-echo filter (5 min) ✓
[5/5] Verify promotion criteria (10 min) ✓
✓ All checks passed
```

---

## Performance Regression Detection

**Automated via CI/CD:**

```bash
# Before each commit, run benchmarks
./tests/perf/run_benchmarks.sh

# Compare against baseline
./tests/perf/compare_to_baseline.sh

# If any metric degrades by >10%, fail the build
```

---

## Test Data

All synthetic test data in `tests/fixtures/`:

```
tests/fixtures/
├── conversations/                 # Pre-recorded agent turns
│   ├── agent_a_auth_exploration.jsonl
│   ├── agent_b_db_schema.jsonl
│   └── agent_c_api_design.jsonl
├── labeled_pairs.json              # 50 pairs for precision test
├── mock_opencode_events.sh         # Helper to emit hook events
└── baseline_metrics.json           # Historical benchmark data
```

---

## Continuous Monitoring (Phase 2+)

Once deployed, track live metrics:

```json
{
  "timestamp": "2026-02-20T15:00:00Z",
  "metric": "injection_latency_p95",
  "value_ms": 18,
  "target_ms": 30,
  "status": "pass"
}
```

**Dashboard:** `~/.impulse/dashboard/` (Zellij plugin, Phase 3)

---

## Cross-References

| Document | Purpose |
|----------|---------|
| `SPEC-v1.1.md` | Historical success metrics document not present in the current workspace |
| [CLI-ARCHITECTURE.md](CLI-ARCHITECTURE.md) | Current surviving architecture-oriented vision reference |
| [DATA-MODELS.md](DATA-MODELS.md) | Query patterns for test validation |
