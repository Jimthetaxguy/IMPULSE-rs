---
title: "ADR-0021: Monty Sandbox for calculator and python_exec"
description: The calculator and python_exec tools run Python inside the in-process Monty interpreter instead of spawning system python3 -c with the daemon's authority
status: review
created: 2026-09-26
updated: 2026-09-26
type: decision
category: architecture
phase: all
audience: builders
deciders: [Impulse Maintainers]
tags: [adr, tooling, sandbox, security, python]
---

# ADR-0021: Monty Sandbox for calculator and python_exec

## Status

Proposed and implemented on lane `agent/claude-monty-calculator-20260926`; accepted on merge.

## Context

`impulse-rs/src/tools/python.rs` backs four surfaces: the `calculator` and `python_exec` dynamic tools
and the `calc` and `exec` CLI commands. `execute_python` spawned system CPython with `python3 -c <code>`.
The child ran with the daemon's full authority: filesystem, network, environment, and process spawning.
The only controls were a 5 second kill timer (PR #66) and, for the calculator alone, a string allowlist
that restricts input to a numeric formula (PR #64). `python_exec` had no restriction at all, and it is
reachable from model output through the tool registry.

Two branches tried to fence CPython with string allowlists. A string filter in front of a full
interpreter is not a sandbox. It has to anticipate every spelling of every dangerous call.

pydantic's Monty is a Python interpreter written in Rust for this case. It has no opcodes for
filesystem, network, or process access. Those capabilities exist only when the host mounts a filesystem
or answers OS calls and host function calls. With none of that wired, the interpreter is the whole
boundary.

## Decision

1. `execute_python` runs code in the in-process `monty` crate:
   `MontyRun::new(code, "exec.py", vec![], CompileOptions::default())`, then
   `run(vec![], ResourceTracker::new(limits), PrintWriter::CollectString(..))`.
   No host functions, no mounts, no inputs. Code goes in; collected print output and a typed result
   come out.
2. Limits: `max_memory` 64 MiB, `max_feed_duration` 5 s (callers may pass a shorter budget), and
   collected print output capped at 16 MiB. `time.sleep` returns at once (`SleepMode::Zero`), so a sleep
   cannot hold the daemon inside the wall-clock budget.
3. Every failure returns a `PythonResult` to the caller. There is no panic path and no fallback to
   CPython. `PythonResult` keeps `output`, `error`, and `exit_code`, and gains
   `fault: Option<&'static str>` holding one of `syntax`, `unsupported`, `runtime`, `timeout`, `memory`.
   The values are exported as constants in `tools::python::fault`.
4. The `python3` spawn is removed from `tools/python.rs` entirely. `is_python_available()` now describes
   the embedded sandbox and is always true; `get_python_version()` reports the Monty version and the
   Python level it implements. The unused `execute_script` helper is deleted. `build_health` keeps its
   own host `python3` PATH probe because that entry describes the host toolchain, not the sandbox.
5. The calculator's math-only input restriction from PR #64 stays. Under Monty it is redundant but
   harmless; removing it is a separate decision.

### Fault classes

| `fault` | Monty exception | Meaning |
| --- | --- | --- |
| `syntax` | `SyntaxError` at compile time | The code did not parse. |
| `unsupported` | `NotImplementedError`, `ModuleNotFoundError` | The code parsed but asks for a Python feature or module the sandbox does not have. `import subprocess` and `import socket` land here (`ModuleNotFoundError`). So do `open()` and `os.getenv()`: with no host handler wired, Monty answers `NotImplementedError: OS function 'open' not implemented with standard execution`. |
| `runtime` | any other exception | An ordinary uncaught exception; the traceback is in `error`. `__import__` is not a defined name in Monty, so `__import__("os")` is a `NameError` here. |
| `timeout` | `TimeoutError` raised by `max_feed_duration` | The wall-clock budget was exhausted. |
| `memory` | `MemoryError` raised by `max_memory` or by the print cap | The memory budget was exhausted. Monty accounts before it allocates: `[0] * (200 * 1024 * 1024)` fails with `memory limit exceeded: 3355443200 bytes > 67108864 bytes` without touching the heap. |

`exit_code` is 0 on success and 1 on any fault, which is what `python3 -c` reported for an uncaught
exception, so existing callers keep working.

Probed end to end through the built `impulse-rs exec` binary on 2026-09-26: `import subprocess`,
`open("/etc/passwd")`, `__import__("os").system("true")`, `os.getenv(...)` (no environment leak),
`import socket`, the 200M-element list, `while True: pass`, and `def (` all came back as faults with
no host effect; `print(2 + 2)` and `calc --expression "2 + 2"` succeeded. `sys.version_info[:2]`
reports `(3, 14)`.

### Toolchain and dependency changes

- `monty` 1.0.0 requires Rust 1.96. `impulse-rs/Cargo.toml` `rust-version` moves from 1.82 to 1.96.
  CI and release workflows use `dtolnay/rust-toolchain@stable`, so no workflow changes. The old floor
  was already nominal: 35 locked dependencies declared a higher MSRV before this change, and under the
  1.82 floor `cargo add monty` silently selected the empty placeholder `monty 0.0.0`.
- `monty` 1.0.0 needs `chrono >= 0.4.40` (it calls `StrftimeItems::new_lenient`) but declares
  `chrono = "0.4"`. The lockfile held chrono at 0.4.39 because the optional `datafusion = "43"`
  dependency pulls `arrow-array 53.4.1`, which caps `chrono < 0.4.40`. No code imports `datafusion`;
  the `datafusion-support` feature only enables the dependency. The requirement moves to
  `datafusion = "55"`, whose arrow tree accepts current chrono. chrono is now 0.4.45.

## Consequences

Model-authored code can no longer read files, open sockets, spawn processes, or read the daemon's
environment. Time and memory limits are enforced by the interpreter instead of by killing a child.
System Python is no longer needed at runtime for these tools.

Costs:

- **No crash isolation.** The interpreter runs inside the daemon process. Monty cannot be made
  crash-proof against a stack-overflow abort or an allocator abort; such a crash takes the whole process
  down. `monty-pool` runs the interpreter in worker subprocesses and turns those crashes into
  `PoolError::Crashed` while the parent stays up. It is the follow-up, not part of this slice.
- Monty implements a subset of Python. Programs that used stdlib modules outside that subset now return
  `fault: unsupported` instead of running.
- The MSRV floor moves to 1.96.

## Follow-ups

1. Move to `monty-pool` with subprocess workers for crash isolation and a parent-side hard timeout.
2. Remove the `impulse-rs/src/monty/` PyO3 stub and the `monty-support` feature, or build them for
   real. They are unbuilt and were wrongly marked Complete in `HANDBOOK.md` (corrected by this lane).
3. Close `agent/grok-calculator-math-only-20260917`; Monty makes that fence unnecessary. PR #64's
   restriction stays for now.
4. Revisit the `security-framework = "=3.6.0"` pin. Its stated reason (the Rust 1.82 MSRV) no longer
   applies.
5. Report the loose `chrono = "0.4"` floor to pydantic/monty.
6. `docs/CLI-COMMANDS.md` says `monty-support` gates `calc` and `exec`. It does not; both run without
   any feature flag.
