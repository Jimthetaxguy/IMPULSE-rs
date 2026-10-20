---
schema: quirewiki-page@1
id: concept.code.impulse-ops
type: concept
title: impulse-ops
status: draft
confidence: high
visibility: public
freshness:
  class: evolving
  review_after: "2026-11-27"
sources:
  - uri: impulse-ops/Cargo.toml
    id: source.c95bcb013af5
    hash: "blake3:b731fdb9b51eeeee977ef73aa711603b54f8c4587036bfdbfeba53ef0ea17a90"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ops/README.md
    id: source.8b5fdcec7c6b
    hash: "blake3:381845b1b9c4078b097b254afa452449d34ae6f9e359e2f85f54887a9445bdf7"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ops/src/agent_registry.rs
    id: source.3fcc4f21640b
    hash: "blake3:873336d2d787eb7557b9e923e2c7f4c4151548dc33795c2315d16f261a1c2f80"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ops/src/governed_task.rs
    id: source.106b30e31d19
    hash: "blake3:d2ad5613e0ddcb3b576b4fb7d4079dcb2e82bad10f324370303e8e2798b10223"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ops/src/lib.rs
    id: source.71fdc7c2f65d
    hash: "blake3:116331a642273a84125ac48bd4e2af309b89ca125f11124d115ca95f76892526"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ops/src/memory_candidate.rs
    id: source.dfede16e7b3c
    hash: "blake3:80b3e60c891a06c2a2ca778803da338b9f5b21710032f63ba8a9c16b4cfd0e83"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ops/src/role_assignment.rs
    id: source.0643a8c97afd
    hash: "blake3:432aa21a3cbae4c52cd31a3fbc51cf83186204c15605f9517a138eb5db894baa"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
claims:
  - id: claim.7a7024eb13ac
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/README.md:3-6"
    source: source.8b5fdcec7c6b
    extract: extract.7a7024eb13ac
  - id: claim.09e0f7e1877b
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/README.md:27-28"
    source: source.8b5fdcec7c6b
    extract: extract.09e0f7e1877b
  - id: claim.29ada40c9860
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/README.md:29-30"
    source: source.8b5fdcec7c6b
    extract: extract.29ada40c9860
  - id: claim.87bc3209c06a
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/README.md:31-32"
    source: source.8b5fdcec7c6b
    extract: extract.87bc3209c06a
  - id: claim.75e036bb4a81
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/README.md:46-47"
    source: source.8b5fdcec7c6b
    extract: extract.75e036bb4a81
  - id: claim.de0be24e8b3b
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/README.md:46-47"
    source: source.8b5fdcec7c6b
    extract: extract.de0be24e8b3b
  - id: claim.20e3b7a83088
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/lib.rs:623-633"
    source: source.71fdc7c2f65d
    extract: extract.78a823793fe5
  - id: claim.4e899769d308
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/lib.rs:593-609"
    source: source.71fdc7c2f65d
    extract: extract.0da0dd95cdc7
  - id: claim.8c6231281ffb
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/lib.rs:636-645"
    source: source.71fdc7c2f65d
    extract: extract.1100f62d4696
  - id: claim.d6946b6f51d9
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/agent_registry.rs:207-215"
    source: source.3fcc4f21640b
    extract: extract.7f7b2ce661e1
  - id: claim.8d9cebf5b1df
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/agent_registry.rs:236-321"
    source: source.3fcc4f21640b
    extract: extract.6d74d5b9b8a8
  - id: claim.387e343c5140
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/agent_registry.rs:323-331"
    source: source.3fcc4f21640b
    extract: extract.c5a4b23af9a9
  - id: claim.f9bd92293128
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/role_assignment.rs:176-202"
    source: source.0643a8c97afd
    extract: extract.a74eb71f33d6
  - id: claim.203118b9aed5
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/role_assignment.rs:229-234"
    source: source.0643a8c97afd
    extract: extract.d8fb3d296920
  - id: claim.27f7b7f70362
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/role_assignment.rs:240-243"
    source: source.0643a8c97afd
    extract: extract.ada3a073cf5d
  - id: claim.1faac642f7d1
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/governed_task.rs:100-106"
    source: source.106b30e31d19
    extract: extract.14d3569f409a
  - id: claim.947ea0f180b8
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/governed_task.rs:149-174"
    source: source.106b30e31d19
    extract: extract.8f2514b4b940
  - id: claim.5f27a46e9b84
    claim_kind: extracted
    confidence: high
    cite: "impulse-ops/src/governed_task.rs:484-503"
    source: source.106b30e31d19
    extract: extract.8a01a0576713
extracts:
  - id: extract.7a7024eb13ac
    text: "Shared, dependency-light **operations contract** crate for the Impulse workspace. It holds the types and protocol constants that more than one crate needs to agree on — the daemon, the desktop host, and the core CLI all depend on `impulse-ops` so they speak the same language without depending on each other."
    text_hash: "sha256:04d7451080c581e2e79c51d34dea3b79acf491727c47f70df13610794aa65dff"
  - id: extract.09e0f7e1877b
    text: "- **Pure contracts only.** Every public type derives `Serialize`/`Deserialize` and has a serde round-trip test. No I/O beyond the two explicit, atomic filesystem helpers."
    text_hash: "sha256:65db8b3b72b75ebdf88df539ed301c4ef22aa60c370fff6216ff7d52499c1420"
  - id: extract.29ada40c9860
    text: "- **No silent failures.** `atomic_write_path` returns `Result<_, OpsError>` (temp file + rename); callers must handle the error."
    text_hash: "sha256:9f087929e7e1cadeaa0624faa36963a02307b2b2bfc9cc6a08c9d5963e460c44"
  - id: extract.87bc3209c06a
    text: "- **Single source of truth.** `agent_registry` is the only place agent platforms and their launch commands are defined; the desktop host and CLI both resolve through it."
    text_hash: "sha256:c093a5dc2e02715999832d0bc6d42afe682bd241f7a5f0007686b41057f0c047"
  - id: extract.75e036bb4a81
    text: "The default suite uses tracked source and fixtures, so fresh clones, linked worktrees, and CI run the same contract proof."
    text_hash: "sha256:0649562ef63ecdba81ae7ff58892b83af4da3785084ca42a179336fd5af2d116"
  - id: extract.de0be24e8b3b
    text: "It is also part of the workspace gate (`cd impulse-rs && cargo test`)."
    text_hash: "sha256:31da61c9be74a7fdd06204a37ea3898bbda878ee37dd96d0b144927d3c179aec"
  - id: extract.78a823793fe5
    text: Convert to legacy String representation for backward compatibility.
    text_hash: "sha256:dfab984fdbd4bc0bfc5e1c22afbfb83c686fe0963381a633125f9d8727d3e5e7"
  - id: extract.0da0dd95cdc7
    text: "Structured status for an agent runtime, replacing plain String. Tracks the agent's current operational state for UI display and coordination."
    text_hash: "sha256:fbaf5fcdc1800ead9a8f906153c840fa086db9ec90251aea6ce24b18eabac543"
  - id: extract.1100f62d4696
    text: Legacy topology role in a coordinator/worker delegation pattern.
    text_hash: "sha256:e83adf726c56318f9bad345791481e7c1ac19a2d8270f7b198e99e74bddcdb19"
  - id: extract.7f7b2ce661e1
    text: "Returns true if `needle` matches this descriptor's id or any alias, case-insensitively."
    text_hash: "sha256:197c6401d89ef5cefc6a4755a3ea5d886e3bad4c3f93ab764222ce1f3371774c"
  - id: extract.6d74d5b9b8a8
    text: "The builtin catalog: the union of every legacy enum's knowledge."
    text_hash: "sha256:de95321082966ad9a943b10f3cfbe0141aa4eb794fa668eedaacc78015af5e79"
  - id: extract.c5a4b23af9a9
    text: "Build a registry from explicit descriptors, validating that every id is non-empty and unique (case-insensitive)."
    text_hash: "sha256:6387a7eca4e8cc2045ec933e5c4f4abf2b83aee835b98a2e897b575ad1fcdf6a"
  - id: extract.a74eb71f33d6
    text: Canonical first product-role contract for a profiled governed launch.
    text_hash: "sha256:055f8275512d7b4ff9d632fb3927c4db45166ccd7b8c1c14a48923a52fe98193"
  - id: extract.d8fb3d296920
    text: Mandatory gaps block launch; optional gaps never do.
    text_hash: "sha256:e2dbda53587eb98d8d084f410692797050561ecacefdf71614483e5715aedd38"
  - id: extract.ada3a073cf5d
    text: A launch is degraded only when it remains allowed with an optional gap.
    text_hash: "sha256:784de65ccf6e4507aa166013a9b77c68bde7b9c886d40b289d0901ff0f676393"
  - id: extract.14d3569f409a
    text: "Closed, versioned producer profiles that the daemon can expand without accepting caller-authored shell commands or evidence."
    text_hash: "sha256:d80820b8b87ca00ef75580b70ac43f7bbe18a1eda3a7f6005521fa7b61c84277"
  - id: extract.8f2514b4b940
    text: Client-proposed durable identity. The daemon validates uniqueness and becomes authoritative for all subsequent revisions.
    text_hash: "sha256:0e8f5513587ea4a33f577c79264f4cc97ec29105e790fb99093d69cd3769a4ef"
  - id: extract.8a01a0576713
    text: Executable name or path with no embedded credentials.
    text_hash: "sha256:91a37ed941f5d3c49b4d7617a8a4653af53f4c6fc66d3b32ac155fc24242e934"
---

# impulse-ops

Shared, dependency-light **operations contract** crate for the Impulse workspace. It holds the types and protocol constants that more than one crate needs to agree on — the daemon, the desktop host, and the core CLI all depend on `impulse-ops` so they speak the same language without depending on each other. (impulse-ops/README.md:3-6)

## What lives here

| Area | Items |
|------|-------|
| **Daemon protocol** | `DAEMON_PROTOCOL_VERSION`, `WorkbenchDaemonRequest`, `WorkbenchDaemonResponse`, `OpsError` |
| **Agent registry** | `agent_registry` module — the single source of truth for known agent platforms (claude-code, codex, …), launch-command resolution, and `AgentPlatformsReport` |
| **Supervisor model** | `SupervisorAction`, `SupervisorProposal`, `SupervisorChatResult`, `SupervisorActionResult`, `SupervisorPermissionPolicy`/`State`, `SupervisorActionPermission`, `PermissionChangeScope`, `ToolCapabilityId` |
| **Agent runtime/coordination** | `AgentRuntime`, `AgentStatus`, `AgentRole`, `MachineTarget`, `DelegationSummary`, `DiffSummary`, `ToolInvocationRecord`, `InterventionRecommendation`, `TerminalOpsReport` |
| **Governed tasks** | Profiled registration, revisioned lifecycle records, claim/verification/review trigger DTOs, and the strict Supervisor response envelope |
| **Accepted-run memory candidates** | Versioned deterministic candidate IDs, pending-review status, source assurance, bounded provenance, and candidate shape validation |
| **Ops snapshot / artifacts** | `ProjectOpsSnapshot`, `OpsEvent`, `OpsSubscription`, `ArtifactEnvelope`, `ArtifactAction`, `ArtifactActionResult`, `ArtifactFileRef`, `ArtifactViewHint`, `ArtifactStatus` |
| **Summaries** | `ProjectSummary`, `MemorySummary`, `RetrievalSummary`, `ContextHealthSummary`, `InsightRecord` |
| **Pure helpers** | `sanitize_id`, `atomic_write_path`, `artifact_store_root` |

Source: (impulse-ops/README.md:13-23)

## Design rules

- **Pure contracts only.** Every public type derives `Serialize`/`Deserialize` and has a serde round-trip test. No I/O beyond the two explicit, atomic filesystem helpers. (impulse-ops/README.md:27-28)
- **No silent failures.** `atomic_write_path` returns `Result<_, OpsError>` (temp file + rename); callers must handle the error. (impulse-ops/README.md:29-30)
- **Single source of truth.** `agent_registry` is the only place agent platforms and their launch commands are defined; the desktop host and CLI both resolve through it. (impulse-ops/README.md:31-32)

## Build & test

```bash
cd impulse-rs/impulse-ops
cargo build && cargo test && cargo clippy -- -D warnings
```

Source: (impulse-ops/README.md:41-44)

The default suite uses tracked source and fixtures, so fresh clones, linked worktrees, and CI run the same contract proof. (impulse-ops/README.md:46-47)
It is also part of the workspace gate (`cd impulse-rs && cargo test`). (impulse-ops/README.md:46-47)

## lib.rs

`to_legacy_string` — Convert to legacy String representation for backward compatibility. (impulse-ops/src/lib.rs:623-633)
`AgentStatus` — Structured status for an agent runtime, replacing plain String. Tracks the agent's current operational state for UI display and coordination. (impulse-ops/src/lib.rs:593-609)
`AgentRole` — Legacy topology role in a coordinator/worker delegation pattern. (impulse-ops/src/lib.rs:636-645)

## src

`matches` — Returns true if `needle` matches this descriptor's id or any alias, case-insensitively. (impulse-ops/src/agent_registry.rs:207-215)
`builtin` — The builtin catalog: the union of every legacy enum's knowledge. (impulse-ops/src/agent_registry.rs:236-321)
`from_descriptors` — Build a registry from explicit descriptors, validating that every id is non-empty and unique (case-insensitive). (impulse-ops/src/agent_registry.rs:323-331)
`canonical_governed_builder_assignment` — Canonical first product-role contract for a profiled governed launch. (impulse-ops/src/role_assignment.rs:176-202)
`launch_allowed` — Mandatory gaps block launch; optional gaps never do. (impulse-ops/src/role_assignment.rs:229-234)
`is_degraded` — A launch is degraded only when it remains allowed with an optional gap. (impulse-ops/src/role_assignment.rs:240-243)
`GovernedVerificationProfile` — Closed, versioned producer profiles that the daemon can expand without accepting caller-authored shell commands or evidence. (impulse-ops/src/governed_task.rs:100-106)
`GovernedTaskRegistration` — Client-proposed durable identity. The daemon validates uniqueness and becomes authoritative for all subsequent revisions. (impulse-ops/src/governed_task.rs:149-174)
`GovernedCommandEvidence` — Executable name or path with no embedded credentials. (impulse-ops/src/governed_task.rs:484-503)

## Sources

- [impulse-ops/Cargo.toml](../../impulse-ops/Cargo.toml)
- [impulse-ops/README.md](../../impulse-ops/README.md)
- [impulse-ops/src/agent_registry.rs](../../impulse-ops/src/agent_registry.rs)
- [impulse-ops/src/governed_task.rs](../../impulse-ops/src/governed_task.rs)
- [impulse-ops/src/lib.rs](../../impulse-ops/src/lib.rs)
- [impulse-ops/src/memory_candidate.rs](../../impulse-ops/src/memory_candidate.rs)
- [impulse-ops/src/role_assignment.rs](../../impulse-ops/src/role_assignment.rs)

## Symbols

- `function` `matches`
- `function` `builtin`
- `function` `from_descriptors`
- `function` `to_legacy_string`
- `function` `canonical_governed_builder_assignment`
- `function` `launch_allowed`
- `function` `is_degraded`
- `enum` `GovernedVerificationProfile`
- `struct` `GovernedTaskRegistration`
- `struct` `GovernedCommandEvidence`
- `enum` `AgentStatus`
- `enum` `AgentRole`
