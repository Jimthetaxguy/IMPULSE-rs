---
schema: quirewiki-page@1
id: concept.code.impulse-ion
type: concept
title: impulse-ion
status: draft
confidence: high
visibility: public
freshness:
  class: evolving
  review_after: "2026-11-27"
sources:
  - uri: impulse-ion/Cargo.toml
    id: source.eaea0d34bae2
    hash: "blake3:a389e576f9c969c4278f812cf5f0e8e8349244ce26244e3ad82a3e1a7eb7fec0"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ion/T5_IMPLEMENTATION_NOTES.md
    id: source.62456817695b
    hash: "blake3:f0d6300ad184482f22d023e7b86883a177e8f1f610036335093e809007fdd248"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ion/TUI_SPEC.md
    id: source.334f53d4c72f
    hash: "blake3:e42d6bd144c2ef90d305a7077d5cbcb94666722d2b378aa98e4437c60d1d87e0"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ion/src/lib.rs
    id: source.5e10d5d746ab
    hash: "blake3:149df343d9e77deefa084fee25f83a3cb207d3ebb9db6abc4e7917f5f3ec4af7"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-ion/src/pi_adapter.rs
    id: source.47e50ee9f5ef
    hash: "blake3:5ab6df17667463f795ca9f3b54f22b3da2956b992f56a21b181838a06ebc1f85"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
claims:
  - id: claim.393567134030
    claim_kind: extracted
    confidence: high
    cite: "impulse-ion/Cargo.toml:6"
    source: source.eaea0d34bae2
    extract: extract.ce4eac4f8eae
  - id: claim.9ff73cd4af32
    claim_kind: extracted
    confidence: high
    cite: "impulse-ion/src/lib.rs:148-188"
    source: source.5e10d5d746ab
    extract: extract.8aa2936f53ef
  - id: claim.5be49fc56f46
    claim_kind: extracted
    confidence: high
    cite: "impulse-ion/src/lib.rs:190-198"
    source: source.5e10d5d746ab
    extract: extract.f1f3f04c0b2c
  - id: claim.d0c2df98b65f
    claim_kind: extracted
    confidence: high
    cite: "impulse-ion/src/lib.rs:221-229"
    source: source.5e10d5d746ab
    extract: extract.e6633ddaef8a
  - id: claim.71b40a3f86f7
    claim_kind: extracted
    confidence: high
    cite: "impulse-ion/src/pi_adapter.rs:73-87"
    source: source.47e50ee9f5ef
    extract: extract.e20e3dfd3232
  - id: claim.6a1d1658df0a
    claim_kind: extracted
    confidence: high
    cite: "impulse-ion/src/pi_adapter.rs:89-96"
    source: source.47e50ee9f5ef
    extract: extract.27a9102c3324
  - id: claim.345c9ec4c877
    claim_kind: extracted
    confidence: high
    cite: "impulse-ion/src/pi_adapter.rs:98-102"
    source: source.47e50ee9f5ef
    extract: extract.1a49bd48ba62
extracts:
  - id: extract.ce4eac4f8eae
    text: Transport-agnostic contracts and process adapter for the Impulse-native Ion runtime
    text_hash: "sha256:328359f09726bd149878220876282aae4bf26d9be812ba912d9391c749bfdd47"
  - id: extract.8aa2936f53ef
    text: "Build a validated verify-intent `HarnessRequest` using the spec-a §2 defaults: read-only `capability_allowlist`, the standard verdict priority order, `model_role = \"verifier-cheap\"`, and a read-only `Context`. This is the single seam for the defaults that used to be hand-rolled at every call site (G4) — callers only supply what varies per request: the repo path, the diff to inspect, and a task description."
    text_hash: "sha256:8e877d8c968ff82f3af0ec1d88fcc413e708851787055d783a7eb67bb3b1cf5a"
  - id: extract.f1f3f04c0b2c
    text: "Validate the write-denial-by-omission rule (spec-a §2, §6)."
    text_hash: "sha256:02a3d62305a2602541f4da8f24ecf3e6c4d2f2a504ef7324aca33eece12cf61a"
  - id: extract.e6633ddaef8a
    text: "Machine-branchable pass/fail per spec-a §5: PASS iff verdict is APPROVE and no CRITICAL finding is present. Callers must never do NLP on prose."
    text_hash: "sha256:a7a2e914d77f0eb388e49e29324c8ff7f204e4b984039f1dff983c3ccb32ab4f"
  - id: extract.e20e3dfd3232
    text: "Resolves the launcher path via env var (`ION_GATE_LAUNCHER`) or the default path under `~/.ai-memory`. See the type-level doc comment for full precedence (explicit arg via `with_launch_script` wins over both)."
    text_hash: "sha256:3e0ec8747c6b9095aafb62c7ff1f87c4bb36c21b974767a685ed3bb9061952c6"
  - id: extract.27a9102c3324
    text: Explicit launcher override (highest precedence — beats the env var and the default path). Primarily for tests driving a stub gate script.
    text_hash: "sha256:6267a2ad27f7cff492744737e0e1a87da8a3358f7dc3848e04ddca27718393e5"
  - id: extract.1a49bd48ba62
    text: "Overrides the child-process timeout (default [`DEFAULT_GATE_TIMEOUT`])."
    text_hash: "sha256:8174d16537c8654f607e4c0b5e912ff22012642619b1bb9095ef7c42ce64abdf"
---

# impulse-ion

Transport-agnostic contracts and process adapter for the Impulse-native Ion runtime. (impulse-ion/Cargo.toml:6)

## lib.rs

`verify` — Build a validated verify-intent `HarnessRequest` using the spec-a §2 defaults: read-only `capability_allowlist`, the standard verdict priority order, `model_role = "verifier-cheap"`, and a read-only `Context`. This is the single seam for the defaults that used to be hand-rolled at every call site (G4) — callers only supply what varies per request: the repo path, the diff to inspect, and a task description. (impulse-ion/src/lib.rs:148-188)
`validate` — Validate the write-denial-by-omission rule (spec-a §2, §6). (impulse-ion/src/lib.rs:190-198)
`passed` — Machine-branchable pass/fail per spec-a §5: PASS iff verdict is APPROVE and no CRITICAL finding is present. Callers must never do NLP on prose. (impulse-ion/src/lib.rs:221-229)

## src

`new` — Resolves the launcher path via env var (`ION_GATE_LAUNCHER`) or the default path under `~/.ai-memory`. See the type-level doc comment for full precedence (explicit arg via `with_launch_script` wins over both). (impulse-ion/src/pi_adapter.rs:73-87)
`with_launch_script` — Explicit launcher override (highest precedence — beats the env var and the default path). Primarily for tests driving a stub gate script. (impulse-ion/src/pi_adapter.rs:89-96)
`with_timeout` — Overrides the child-process timeout (default [`DEFAULT_GATE_TIMEOUT`]). (impulse-ion/src/pi_adapter.rs:98-102)

## Sources

- [impulse-ion/Cargo.toml](../../impulse-ion/Cargo.toml)
- [impulse-ion/T5_IMPLEMENTATION_NOTES.md](../../impulse-ion/T5_IMPLEMENTATION_NOTES.md)
- [impulse-ion/TUI_SPEC.md](../../impulse-ion/TUI_SPEC.md)
- [impulse-ion/src/lib.rs](../../impulse-ion/src/lib.rs)
- [impulse-ion/src/pi_adapter.rs](../../impulse-ion/src/pi_adapter.rs)

## Symbols

- `function` `verify`
- `function` `validate`
- `function` `passed`
- `function` `new`
- `function` `with_launch_script`
- `function` `with_timeout`
