# Sandbox Integration — Impulse Spec

> **Date:** 2026-03-02
> **Status:** Draft
> **Companion:** Private local design note, not included in this public repository.
> **Source:** NullClaw analysis §2.4, existing Impulse guardrail system

---

## 1. Current State

### Guardrail Types (`src/guardrail/types.rs`)

```rust
// Line 10 — What to do when a rule matches
pub enum GuardAction {
    Block,  // Stop the operation
    Warn,   // Alert but allow
    Log,    // Silent record
}

// Line 51 — What the rule applies to
pub enum GuardTarget {
    Bash,       // Shell commands
    ToolCall,   // Tool invocations
    FileWrite,  // File write operations
    Any,        // Matches everything
}

// Line 91 — A single guardrail rule
pub struct GuardRule {
    pub id: String,
    pub pattern: String,         // Regex pattern
    pub action: GuardAction,
    pub target: GuardTarget,
    pub reason: String,
    pub suggestion: Option<String>,
    pub enabled: bool,
    pub builtin: bool,
}

// Line 142 — Overall guardrail config
pub struct GuardConfig {
    pub enabled: bool,
    pub rules: Vec<GuardRule>,
}
```

### What Exists

The guardrail system matches regex patterns against command strings and takes
Block/Warn/Log actions. This is **pattern-level** defense — it inspects the command
text before execution.

### What's Missing

No OS-level containment. Once a command passes the guardrail regex check, it runs
with the user's full permissions. The guardrail system cannot:
- Restrict file system access (a command could read `~/.ssh/id_rsa`)
- Limit network access (a command could exfiltrate data)
- Prevent privilege escalation
- Enforce resource limits (CPU, memory)

---

## 2. Integration Points

Three places in Impulse spawn external processes:

### Spawn Point 1: CliAgent (`src/llm_backends/cli.rs`)

`CliAgent` at line 65 spawns CLI tools (Claude Code, OpenCode) as subprocesses via
`tokio::process::Command`. The `start_session()` method at line 161 builds and spawns
the command.

```
  Current flow:
  CliAgent.start_session()
      -> Command::new(cmd)
      -> command.spawn()

  Proposed flow:
  CliAgent.start_session()
      -> Command::new(cmd)
      -> sandbox.wrap_command(argv, profile)   <-- NEW
      -> Command::new(wrapped_argv[0])
      -> command.spawn()
```

### Spawn Point 2: PTY Backend (`impulse-term/src/backend.rs`)

The terminal emulator spawns a shell process via PTY. This is the primary interactive
shell that users see in Impulse panes.

```
  Current flow:
  PtyBackend::spawn(shell_cmd)
      -> pty::fork()
      -> run(shell_cmd)

  Proposed flow:
  PtyBackend::spawn(shell_cmd)
      -> sandbox.wrap_command([shell_cmd], profile)   <-- NEW
      -> pty::fork()
      -> run(wrapped_cmd)
```

### Spawn Point 3: Daemon Mode (Future)

When Impulse runs as a daemon managing agent processes, each spawned agent should
be sandboxed independently.

### Integration Diagram

```
  +--------------------------------------------------+
  |                  Impulse Core                     |
  |                                                  |
  |  +-------------+  +--------------------------+   |
  |  | GuardConfig |  | SandboxManager           |   |
  |  | (pattern)   |  |                          |   |
  |  |             |  |  backend: Box<dyn Sandbox>|  |
  |  |  rules[]    |  |  config: SandboxConfig   |   |
  |  |  evaluate() |  |  audit_log: PathBuf      |   |
  |  +------+------+  +-----------+--------------+   |
  |         |                     |                   |
  |         v                     v                   |
  |    +-----------------------------------------+    |
  |    |         Command Execution Path          |    |
  |    |                                         |    |
  |    |  1. guardrail.evaluate(cmd)             |    |
  |    |     -> Block? STOP                      |    |
  |    |     -> Warn? log + continue             |    |
  |    |                                         |    |
  |    |  2. sandbox.wrap_command(argv, profile)  |   |
  |    |     -> wrapped_argv                     |    |
  |    |                                         |    |
  |    |  3. Command::new(wrapped_argv[0])       |    |
  |    |     .args(wrapped_argv[1..])            |    |
  |    |     .spawn()                            |    |
  |    |                                         |    |
  |    |  4. audit.log(cmd, outcome, timestamp)  |    |
  |    +-----------------------------------------+    |
  +--------------------------------------------------+
```

---

## 3. Module Structure

```
  src/sandbox/
  +-- mod.rs            # SandboxManager, auto-detection, re-exports
  +-- types.rs          # Sandbox trait, SandboxConfig, SandboxProfile
  +-- detect.rs         # detect_best_backend() -> Box<dyn Sandbox>
  +-- sandbox_exec.rs   # macOS sandbox-exec backend
  +-- docker.rs         # Docker container backend
  +-- noop.rs           # NoopBackend (passthrough, always available)
  +-- profiles/
      +-- restrictive.rs  # Deny-all + allowlist
      +-- permissive.rs   # Allow-all + denylist
```

---

## 4. SandboxManager

```rust
pub struct SandboxManager {
    /// The selected sandbox backend
    backend: Box<dyn Sandbox>,
    /// Sandbox configuration
    config: SandboxConfig,
    /// Path to audit log file
    audit_log: PathBuf,
}

impl SandboxManager {
    /// Auto-detect the best available backend
    pub async fn detect(config: SandboxConfig) -> Self {
        let backend = detect::detect_best_backend().await;
        let audit_log = config.audit_log_path.clone()
            .unwrap_or_else(|| PathBuf::from(".impulse/sandbox-audit.jsonl"));
        Self { backend, config, audit_log }
    }

    /// Wrap a command for sandboxed execution
    pub fn wrap(&self, argv: &[String]) -> Result<Vec<String>> {
        if !self.config.enabled {
            return Ok(argv.to_vec());
        }

        let profile = self.resolve_profile(argv);
        let wrapped = self.backend.wrap_command(argv, &profile)?;

        self.audit_log_entry(argv, &wrapped);
        Ok(wrapped)
    }

    /// Get the current backend name
    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }

    /// Resolve which profile to use for a given command
    fn resolve_profile(&self, _argv: &[String]) -> SandboxProfile {
        self.config.default_profile.clone()
    }

    /// Write a JSON audit log entry
    fn audit_log_entry(&self, original: &[String], wrapped: &[String]) {
        // Append JSONL: { "timestamp", "original", "wrapped", "backend", "profile" }
    }
}
```

---

## 5. Config

New `sandbox` key in Impulse's `config.json`:

```json
{
  "sandbox": {
    "enabled": false,
    "backend_override": null,
    "default_profile": "permissive",
    "audit_log_path": ".impulse/sandbox-audit.jsonl",
    "allowed_paths": [
      "/usr/bin",
      "/usr/lib",
      "{{project_dir}}"
    ],
    "denied_paths": [
      "~/.ssh",
      "~/.gnupg",
      "~/.aws",
      "~/.config/gh"
    ]
  }
}
```

When `sandbox.enabled` is `false` (default), `SandboxManager.wrap()` returns argv
unchanged — zero overhead, zero behavioral change.

---

## 6. Dual-Mode Operation

```
  +----------------+--------------------------+------------------------+
  | Mode           | What Gets Sandboxed      | Profile                |
  +----------------+--------------------------+------------------------+
  | Direct         | Individual commands from  | Permissive (CLI tools  |
  | (CliAgent)     | CliAgent.start_session() | need broad access)     |
  |                | e.g., "claude --print"   |                        |
  +----------------+--------------------------+------------------------+
  | Terminal       | PTY shell commands from   | Configurable per-pane  |
  | (impulse-term) | PtyBackend::spawn()      | (restrictive for       |
  |                | e.g., "/bin/zsh"         | untrusted agents)      |
  +----------------+--------------------------+------------------------+
  | Daemon         | Spawned agent processes   | Restrictive (agents    |
  | (future)       | in headless mode         | get minimal access)    |
  +----------------+--------------------------+------------------------+
```

---

## 7. Sandbox Backend Implementations

Three backends, each implementing the `Sandbox` trait:

**SandboxExecBackend** (macOS) — Wraps commands with `sandbox-exec -f profile.sb`.
Checks `cfg!(target_os = "macos")` and `which sandbox-exec` for availability.
Generates .sb profile files from `SandboxProfile` allowed/denied paths.

**DockerBackend** — Wraps commands with `docker run --rm --network=none`. Mounts
allowed read paths as `:ro` volumes, write paths as read-write. Checks
`docker version` for availability.

**NoopBackend** — Returns argv unchanged. Always available. Used as fallback when
no real sandbox is detected.

---

## 8. Migration Path

### Phase 1: Add Sandbox Trait + NoopBackend

- Create `src/sandbox/` directory with `types.rs`, `noop.rs`, `mod.rs`
- `SandboxManager` always selects NoopBackend
- Wire into config (disabled by default)
- No behavioral change

### Phase 2: Implement detect + sandbox-exec

- Add `detect.rs` and `sandbox_exec.rs`
- Auto-detect sandbox-exec on macOS
- Generate basic .sb profile files
- Test with simple commands

### Phase 3: Wire into CliAgent

- Inject `SandboxManager` into `CliAgent` construction
- `start_session()` calls `sandbox.wrap()` before spawning
- Audit log records all sandboxed CLI agent commands

### Phase 4: Wire into impulse-term

- PTY spawn path calls `sandbox.wrap()` before fork
- Per-pane profile configuration
- UI indicator showing sandbox status

### Phase 5: Add Docker Backend

- Implement `DockerBackend` with volume mounting
- Docker detection in auto-detect chain
- Network isolation by default

### Phase 6: Add Audit System

- JSONL audit log with timestamps, commands, outcomes
- `impulse sandbox audit` CLI command to review log
- Retention policy (rotate after N days or N MB)

---

## 9. Testing Strategy

| Test | Type | Description |
|------|------|-------------|
| `test_noop_passthrough` | Unit | NoopBackend returns argv unchanged |
| `test_noop_always_available` | Unit | NoopBackend.is_available() returns true |
| `test_sandbox_exec_wrap` | Unit | Verify -f profile.sb prefix |
| `test_sandbox_exec_profile` | Unit | Profile generates valid .sb syntax |
| `test_docker_wrap_readonly` | Unit | Read-only paths get :ro flag |
| `test_docker_no_network` | Unit | --network=none present |
| `test_detect_macos` | Integration | On macOS selects sandbox-exec or Docker |
| `test_detect_linux` | Integration | On Linux selects Docker or Noop |
| `test_detect_no_backend` | Unit | Nothing available selects NoopBackend |
| `test_config_disabled` | Unit | enabled=false returns argv unchanged |
| `test_denied_paths` | Unit | Denied paths not in allowed volumes |
| `test_audit_jsonl` | Unit | wrap() writes valid JSONL entry |
| `test_backward_compat` | Integration | sandbox.enabled=false is zero change |

---

## 10. Cross-References

- **Companion (cross-cutting):** Private local design note, not included in this public repository.
- **Spec 4 — Agent Patterns:** [`spec-nullclaw-agent-patterns.md`](./spec-nullclaw-agent-patterns.md) — vtable comparison informs trait design
- **Impulse source:**
  - `src/guardrail/types.rs:10` — `GuardAction` enum (Block/Warn/Log)
  - `src/guardrail/types.rs:51` — `GuardTarget` enum (Bash/ToolCall/FileWrite/Any)
  - `src/guardrail/types.rs:91` — `GuardRule` struct (pattern matching)
  - `src/guardrail/types.rs:142` — `GuardConfig` struct
  - `src/llm_backends/cli.rs:65` — `CliAgent` (spawn point 1)
  - `src/llm_backends/cli.rs:161` — `start_session()` subprocess spawn
  - `impulse-term/src/backend.rs` — PTY backend (spawn point 2)
- **NullClaw source:**
  - `src/security/sandbox.zig` — Sandbox vtable (4 operations)
  - `src/security/detect.zig` — Auto-detection priority chain
  - `src/security/landlock.zig` — Linux kernel sandboxing
  - `src/security/audit.zig` — JSON audit logging
- **Hookify rules:** `~/.claude/hookify.block-*.local.md`
