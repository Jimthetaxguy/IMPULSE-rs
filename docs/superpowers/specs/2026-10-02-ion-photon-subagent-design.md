---
title: Ion photon — a disposable read-only subagent tool
description: Package the min-agent read-only coding agent as an Ion ReplTool that answers one question about a workspace subtree and is then thrown away
updated: 2026-10-02
type: specification
category: architecture
phase: all
status: active
audience: builders
tags: [ion, subagent, min-agent, tools, sandbox]
---

# Ion `photon`: a disposable read-only subagent

Author: Claude (Opus 5.5), 2026-10-02, at James's direction ("package a min-agent like
min-agent-rs as a tool … a very disposable subagent"). Lane card:
`docs/plans/worktrees/2026-10-02-claude-ion-photon-subagent.md`.

## Name

Renamed from `scout` on 2026-10-03 to match the physics naming of Impulse (a change in momentum)
and Ion (a charged particle). A photon is emitted, carries information, has no mass, leaves nothing
behind, and is absorbed: the subagent is sent out, reads without side effects, returns one answer,
and is gone.

## Intent

Ion's model sometimes needs an answer about a part of the repository that would cost many of its
own tool rounds and a lot of its own context to find: "where is the daemon socket bounded?",
"which tests cover settlement?". A **photon** hands that question to a fresh, cheap, read-only
agent, gets back one answer with the evidence it used, and forgets the run. Nothing the photon did
survives the call except the returned result.

`min-agent` (`github.com/Jimthetaxguy/min-agent-rs`, v0.2.0) already has that shape: one public
`run(client, workspace, prompt, options, trace) -> RunReport`, a `cap-std` workspace capability
with three read tools, a full `Budget`, and a typed `StopReason` where only `Completed` is
success. This design wraps it; it does not change it.

## Contract

```text
photon { question: string, path?: string } -> ToolOutcome
  ok       = stop is Completed
  rendered = the answer, or "photon stopped: <reason>" plus any partial text labeled as not an answer
  payload  = the serialized RunReport (stop, answer, counts, usage, per-call evidence; no transcript)
```

The model chooses only the question and an optional subdirectory. The host chooses the model,
the connection, the budget, and the root.

## Rules

1. **Disposable.** Each call builds a new client, workspace capability, and transcript, and drops
   them on return. No trace file, no memory or genome write, no session. Spec C of the Ion
   harness series ("no durable memory authority") holds by construction.
2. **Read-only and scoped.** The root is `repo_root` or a directory inside the session's read
   sandbox (`ReplContext::sandbox_tool_context`). It must be a directory. `min-agent`'s own
   policy then applies inside it: no traversal, symlinks, credential files, `.git`, `target`, or
   `node_modules`, and credential-shaped content is redacted.
3. **Budget fits inside the parent.** The photon's wall clock is half of Ion's tool-loop budget
   (`ION_DEFAULT_WALL_CLOCK / 2`), so one photon cannot consume the whole parent exchange. A photon
   is one tool call to the parent loop.
4. **Host-owned model choice** (ADR-0015). Anthropic Messages at the same origin Ion uses
   (`ANTHROPIC_BASE_URL` or the canonical default), credential from `ANTHROPIC_API_KEY` by name,
   model from `ION_PHOTON_MODEL` or a small default. Nothing in the tool schema selects a model.
5. **Untrusted output.** The answer is model text derived from file contents. It goes through the
   existing untrusted tool-output envelope and `GuardTarget::ToolCall` scan like every tool
   result, so a hostile file cannot approve a later gated call.
6. **Spend cap instead of a confirmation prompt.** The tool has no side effects, so it stays
   outside `CONFIRMATION_REQUIRED_TOOLS`, like `file_read` and `ion_verify`. It does spend model
   tokens, so each registry (one per REPL session) allows at most `PHOTON_SESSION_LIMIT` runs.
   Refused calls (bad arguments, sandbox escape) do not consume a slot.
7. **No partial answer as success.** A budget, provider, or protocol stop returns `ok: false`.

## Packaging

- `min-agent` is an optional git dependency pinned to a commit, enabled by the default
  `photon-subagent` feature. Turning the feature off removes the tool and the dependency.
- `min_agent::agent::run` is blocking; the tool runs it on `tokio::task::spawn_blocking`.
  The run cannot be cancelled mid-flight; its own wall-clock deadline is the backstop.
- The model-client factory is injected, so tests drive the real loop with a scripted
  `ModelClient` and production uses `HttpModelClient`.

## Out of scope

- Parallel photons (overlaps ADR-0014 comparative settlement).
- A stdio/MCP surface so Claude Code or Codex can call the same photon. That depends on
  `min-agent` lifting its "MCP deferred by design" stance.
- Use from the governed Supervisor, which must stay tool-free (ADR-0012).
- Carving the photon deadline from the parent's *remaining* time. Ion tools do not receive the
  loop deadline today; the static half-budget is the bound until they do.

## Tests

Happy path with a scripted client reading a planted file; argument validation; sandbox escape and
non-directory roots refused without consuming a slot; session cap; budget stop is not success;
budget fits inside the parent; production connection uses Anthropic Messages, the named
credential env, and the model override; registry includes the tool only with the feature; the
tool stays outside confirmation.
