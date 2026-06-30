# Plan: State Management Enhancements

> **STATUS: ✅ COMPLETE (verified 2026-06-26).** All six steps below have landed. Evidence in the
> current tree: `state/mod.rs` is now a 15-line module facade; the daemon `process_request()` lives
> in `daemon/handlers.rs` (~173 lines, a thin dispatcher) with boundary validation wired
> (`validate::validate_session_id`/`validate_tool_id`/`reject_control_chars`); `respond_ok` exists in
> `daemon/protocol.rs`; `evaluate_track_guardrails` in `handlers/session.rs`; and Config get/set/list
> is driven by the `state/config_keys/` registry (`SetRule`, `build_set_rules`, `set_field_json`).
> The dead `State::sync`/`LiveState::active_sessions`/`State::get_history` methods are gone. This
> document is retained as the historical design record for that refactor.

## Summary

Six incremental steps to simplify state management. Each step builds and tests independently. Total estimated savings: ~700 lines removed, daemon `process_request()` drops from 815 → ~50 lines, Config get/set/list collapses from ~1,000 → ~100 lines.

---

## Step 1: Remove dead code from state/mod.rs

Remove 3 methods marked `#[allow(dead_code)]` that are never called:

| Method | Why dead |
|--------|---------|
| `State::sync()` | `sync_immediate()` is the only sync path; `sync()` is never called |
| `LiveState::active_sessions()` | Never called anywhere |
| `State::get_history()` | `get_history_sync()` is the used variant |

Also remove any `#[allow(dead_code)]` annotations left behind.

**Files:** `impulse-rs/src/state/mod.rs`
**Est. savings:** ~35 lines
**Risk:** Zero — dead code removal

---

## Step 2: Extract guardrail evaluation helper in session handlers

`handle_track_write` and `handle_track_tool` contain identical 15-line guardrail blocks. Extract to shared helper:

```rust
async fn evaluate_track_guardrails(
    state: &Arc<state::State>,
    session_id: &str,
    action: &str,
) -> Result<()> {
    if let Ok(config) = state.config_snapshot() {
        if config.guardrails.enabled {
            if let Ok(results) = guardrail::evaluate_action(action, "any", &config.guardrails) {
                for result in &results {
                    match result.action {
                        guardrail::GuardAction::Warn => eprintln!("{}", result.format_human()),
                        guardrail::GuardAction::Log => {
                            let _ = state.add_tag(session_id, &format!("guard:{}", result.rule_id)).await;
                        }
                        guardrail::GuardAction::Block => {}
                    }
                }
            }
        }
    }
    Ok(())
}
```

Then both handlers call `evaluate_track_guardrails(&state, &sid, &file).await?;`

**Files:** `impulse-rs/src/handlers/session.rs`
**Est. savings:** ~15 lines

---

## Step 3: Extract daemon response helpers

Extract 3 repeated patterns from `process_request()`:

```rust
/// Wrap a serializable value in DaemonResponse::Ok (used ~15x)
fn respond_ok<T: Serialize>(value: &T) -> DaemonResponse {
    match serde_json::to_value(value) {
        Ok(result) => DaemonResponse::Ok { result },
        Err(e) => DaemonResponse::Error { message: format!("serialize: {e}") },
    }
}

/// Get config snapshot or return DaemonResponse::Error (used 6x)
fn config_snapshot_or_err(state: &State) -> Result<Config, DaemonResponse> {
    state.config_snapshot().map_err(|e| DaemonResponse::Error { message: e.to_string() })
}
```

**Files:** `impulse-rs/src/daemon/mod.rs`
**Est. savings:** ~60 lines

---

## Step 4: Split daemon process_request into sub-handlers

Split the 815-line match into 6 async functions. `process_request()` becomes a thin dispatcher:

```rust
async fn process_request(...) -> DaemonResponse {
    match request {
        DaemonRequest::Ping => DaemonResponse::Ok { result: json!({"pong": true}) },
        DaemonRequest::Status => handle_status(state).await,

        // Session group
        DaemonRequest::CreateSession { .. }
        | DaemonRequest::EndSession { .. }
        | DaemonRequest::GetSession { .. }
        | DaemonRequest::ListSessions
        | DaemonRequest::TrackFile { .. }
        | DaemonRequest::TrackTool { .. }
        | DaemonRequest::CheckConflict { .. } => {
            handle_session_request(request, state).await
        }

        // Tool group
        DaemonRequest::ListTools | ... => handle_tool_request(request, state, registry, tool_context).await,

        // Steward group
        DaemonRequest::StewardStatus | ... => handle_steward_request(request, state).await,

        // Supervisor group
        DaemonRequest::SupervisorChat { .. } | ... => handle_supervisor_request(...).await,

        // Ops group
        DaemonRequest::GetOpsSnapshot | ... => handle_ops_request(...).await,

        // Guard group
        DaemonRequest::GuardEvaluate { .. } | ... => handle_guard_request(request, state).await,
    }
}
```

All sub-handlers stay in `daemon/mod.rs` (no new files). Each takes only the parameters it needs.

**Files:** `impulse-rs/src/daemon/mod.rs`
**Est. savings:** Net ~0 (restructure), but `process_request()` drops from 815 → ~50 lines

---

## Step 5: Add daemon boundary validation

Wire `validate` module into `process_request()` before dispatch. Validate IDs extracted from request variants:

```rust
// In process_request, before dispatching session requests:
if let DaemonRequest::EndSession { ref session_id, .. }
    | DaemonRequest::GetSession { ref session_id }
    | DaemonRequest::TrackFile { ref session_id, .. }
    | DaemonRequest::TrackTool { ref session_id, .. }
    | DaemonRequest::CheckConflict { ref session_id, .. } = request
{
    if let Err(e) = validate::validate_session_id(session_id) {
        return DaemonResponse::Error { message: e.to_string() };
    }
}
```

**Files:** `impulse-rs/src/daemon/mod.rs`
**Est. addition:** ~25 lines

---

## Step 6: Config get/set/list via serde reflection + validation registry

**This is the highest-impact step.** Replace ~1,000 lines of hand-written match arms with:

### get() — serde reflection (zero-maintenance)

```rust
pub fn get(&self, key: &str) -> Option<String> {
    let value = serde_json::to_value(self).ok()?;
    let obj = value.as_object()?;
    let v = obj.get(key)?;
    Some(match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => return None,
        other => other.to_string(),
    })
}
```

Adding a new field to Config automatically makes it available via `get()` — zero additional code.

### list() — serde reflection (zero-maintenance)

```rust
pub fn list(&self) -> Vec<(String, String)> {
    let value = serde_json::to_value(self).unwrap_or_default();
    let obj = match value.as_object() {
        Some(o) => o,
        None => return vec![],
    };
    obj.iter()
        .map(|(k, v)| {
            let display = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => "<not set>".to_string(),
                other => other.to_string(),
            };
            (k.clone(), display)
        })
        .collect()
}
```

### set() — validation registry

The 69 fields break down into 5 simple patterns + 3 custom fields:

| Pattern | Count | Registry entry |
|---------|-------|----------------|
| bool_parse | 26 | `SetRule::Bool` |
| numeric with range | 24 | `SetRule::Numeric { min, max }` (for u64/usize/f64/f32/u32) |
| enum allowlist | 9 | `SetRule::Enum(&[...])` |
| string direct | 8 | `SetRule::String` |
| vec_csv | 5 | `SetRule::CsvList` |
| platform_enum | 1 | `SetRule::Custom(fn)` |
| custom | 3 | `SetRule::Custom(fn)` |

```rust
enum SetRule {
    Bool,
    String,
    CsvList,
    Numeric { min: f64, max: f64, kind: NumericKind },
    Enum(&'static [&'static str]),
    Custom(fn(&mut Config, &str) -> bool),
}

enum NumericKind { U64, Usize, U32, F32, F64 }

fn build_set_rules() -> HashMap<&'static str, SetRule> {
    let mut m = HashMap::new();
    // Bool fields (26)
    m.insert("verbose", SetRule::Bool);
    m.insert("retrieval_vector_enabled", SetRule::Bool);
    // ... all 26

    // Numeric fields (24)
    m.insert("sync_interval_secs", SetRule::Numeric { min: 0.0, max: f64::MAX, kind: NumericKind::U64 });
    m.insert("retrieval_similarity_threshold", SetRule::Numeric { min: 0.0, max: 1.0, kind: NumericKind::F32 });
    // ... all 24

    // Enum fields (9)
    m.insert("log_level", SetRule::Enum(&["trace","debug","info","warn","error"]));
    m.insert("retrieval_mode", SetRule::Enum(&["keyword","semantic"]));
    // ... all 9

    // String fields (8)
    m.insert("retrieval_embedding_provider", SetRule::String);
    // ... all 8

    // CSV list fields (5)
    m.insert("build_hygiene_scan_paths", SetRule::CsvList);
    // ... all 5

    // Custom (4 - platform + 3 complex)
    m.insert("default_platform", SetRule::Custom(set_platform));
    m.insert("impulse_agent_provider", SetRule::Custom(set_agent_provider));
    m.insert("impulse_agent_harness", SetRule::Custom(set_agent_harness));
    m.insert("impulse_agent_permissions", SetRule::Custom(set_agent_permissions));
    m
}

pub fn set(&mut self, key: &str, value: &str) -> bool {
    let rules = build_set_rules();
    let Some(rule) = rules.get(key) else { return false };

    match rule {
        SetRule::Bool => {
            self.set_field_json(key, serde_json::Value::Bool(value.parse().unwrap_or(false)))
        }
        SetRule::String => {
            if value.is_empty() { return false; }
            self.set_field_json(key, serde_json::Value::String(value.to_string()))
        }
        SetRule::CsvList => {
            let items: Vec<String> = value.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            self.set_field_json(key, serde_json::to_value(items).unwrap_or_default())
        }
        SetRule::Numeric { min, max, kind } => {
            let f: f64 = value.parse().ok()?;
            if f < *min || f > *max { return false; }
            let json_val = match kind { /* convert to appropriate JSON type */ };
            self.set_field_json(key, json_val)
        }
        SetRule::Enum(allowed) => {
            if !allowed.contains(&value) { return false; }
            self.set_field_json(key, serde_json::Value::String(value.to_string()))
        }
        SetRule::Custom(f) => f(self, value),
    }
}

/// Apply a single field change via serde round-trip
fn set_field_json(&mut self, key: &str, val: serde_json::Value) -> bool {
    let mut obj = match serde_json::to_value(&*self) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => return false,
    };
    obj.insert(key.to_string(), val);
    match serde_json::from_value(serde_json::Value::Object(obj)) {
        Ok(new_config) => { *self = new_config; true }
        Err(_) => false,
    }
}
```

**Why this is the most robust approach:**
- `get()` and `list()` are **zero-maintenance** — adding a Config field just works
- `set()` validation is **explicit and debuggable** — each field has a declarative rule in a HashMap, not hidden in a macro expansion
- `set_field_json()` uses serde round-trip which **guarantees type safety** — if the JSON value doesn't match the Rust type, `from_value` fails cleanly
- No macros = standard Rust debugging, error messages, and IDE support
- The validation registry is ~80 lines replacing ~575 lines of match arms

**Files:** `impulse-rs/src/state/mod.rs`
**Est. savings:** ~550 lines (575 lines of set() match + 120 lines of get() match + 285 lines of list() match → ~100 lines of registry + ~50 lines of implementation)

---

## Implementation Order

```
Step 1: Dead code removal          (safe, zero-risk)
Step 2: Guardrail helper           (small, self-contained)
Step 3: Daemon response helpers    (prerequisite for Step 4)
Step 4: Daemon request split       (biggest structural improvement)
Step 5: Daemon boundary validation (builds on Step 4)
Step 6: Config serde+registry      (highest line savings, most complex)
```

## Verification

After each step:
```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings
```
