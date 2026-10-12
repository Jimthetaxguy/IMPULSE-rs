---
title: "ADR-0010: Product Role Launch Contract"
status: accepted
created: 2026-07-13
deciders: [Impulse Maintainers]
---

# ADR-0010: Product Role Launch Contract

## Status

Accepted.

## Context

Impulse needs one honest launch boundary before it can claim a complete governed
supervisor/builder workflow. A launch must identify the product role and task, compare the role's
requirements with conservative runtime support, and reject mandatory gaps before a process exists.
The cockpit may explain that comparison, but it cannot be the enforcement authority.

The repository already has an `AgentRole` enum for coordinator/worker pane and delegation topology.
Repurposing that compatibility type as the product role would conflate presentation topology with
behavioral obligations and permissions. Likewise, setting a process working directory is useful
mediation, but it is not filesystem isolation and must never be presented as a sandbox.

## Decision

Adopt the following narrow, backward-compatible product-role launch contract:

1. **Product role identity is distinct from legacy topology.** `AgentRoleId` is the open,
   validated identifier for a product role. The existing `AgentRole::{Coordinator, Worker}` remains
   the legacy coordinator/worker topology field and is not a product-role policy contract.
2. **An explicit task accompanies every governed launch.** A request carrying an
   `AgentRoleAssignment` must also carry a nonblank task. Assignment-free legacy launch requests
   remain valid.
3. **Launch compatibility uses trusted, code-owned declarations.** Capability support is static
   Rust-owned registry metadata, not a runtime self-report, prompt claim, terminal observation,
   operator TOML attestation, or probe of model-internal behavior. The evaluator compares the
   caller-supplied role requirements with the selected canonical platform's declared launch
   support.
4. **Enforcement strength is ordered and explicit.** The four strengths are `unsupported`,
   `advisory`, `mediated`, and `structural`. Missing support is `unsupported`. A mandatory gap
   blocks launch; an optional gap permits launch but marks it degraded.
5. **Dioxus previews the same compatibility model and fails closed.** The current Dioxus launcher
   supplies a fixed initial Builder assignment and explicit task, renders required versus available
   strength, and disables launch for a blank task or mandatory incompatibility. This is a cockpit
   preview, not authority or a generalized role catalog.
6. **The backend is the authoritative pre-PTY gate.** `DesktopRuntime::spawn_agent` canonicalizes
   the platform, re-evaluates compatibility, and rejects a mandatory gap before agent-id reservation
   or PTY creation. UI state cannot bypass or weaken this check.
7. **Working-directory control is not a sandbox.** Built-in launch metadata may describe workspace
   targeting and process lifecycle as `mediated`. It must leave scoped-filesystem structural
   enforcement unsupported unless a real sandbox boundary proves otherwise.
8. **Typed launch facts remain observable.** The explicit task, product-role assignment, and
   compatibility result flow through runtime snapshots and existing daemon terminal-operations
   telemetry. Older payloads that omit the new fields continue to deserialize, and older telemetry
   must not erase newer typed facts.

This ADR accepts only the static launch-preflight slice. It does not establish generalized role
composition, a common runtime-adapter trait, runtime probing or capability negotiation, durable task
lifecycle, model-internal governance, supervisor judgment, evidence acceptance, verification
decisions, or the complete governed vertical slice.

## Consequences

- Product-role and capability identifiers can expand without changing the legacy topology enum.
- The fixed Dioxus Builder profile is a UI-owned initial profile; the backend evaluates the
  requirements supplied by its caller and is not yet the canonical owner of role composition.
- Built-in external-runtime launches can honestly be allowed-but-degraded when optional structural
  filesystem enforcement is unavailable.
- Compatibility is a conservative preflight over declared launch conditions, not proof that a
  model follows a role after launch.
- Existing assignment-free launch payloads and legacy topology fields remain compatible.
- The next governed slice must move beyond launch into a daemon-owned governed-run lifecycle that
  records task evidence, verification results, and supervisor accept/reject/escalate decisions
  before any outcome is treated as complete or promoted to durable memory.

## Validation

This decision is represented when:

1. Product-role assignment and legacy `AgentRole` topology remain separate serialized fields.
2. The Dioxus preview and backend evaluator expose the same ordered strengths and mandatory/optional
   semantics.
3. The backend rejects an incompatible governed request before reserving an agent id or spawning a
   PTY.
4. Workspace mediation is never described as filesystem sandboxing.
5. Assignment-free legacy payloads still deserialize and typed launch facts survive runtime and
   daemon telemetry conversion.

## Related Documents

- [`../../VISION.md`](../../VISION.md)
- [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md)
- [`../ARCHITECTURE-CLARIFICATION.md`](../ARCHITECTURE-CLARIFICATION.md)
- [`../superpowers/plans/2026-07-12-governed-role-launch.md`](../superpowers/plans/2026-07-12-governed-role-launch.md)
