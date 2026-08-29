---
schema: quirewiki-page@1
id: concept.code.impulse-step-model
type: concept
title: impulse-step-model
status: draft
confidence: high
visibility: public
freshness:
  class: evolving
  review_after: "2026-11-27"
sources:
  - uri: impulse-step-model/Cargo.toml
    id: source.0176b371df81
    hash: "blake3:03e7bde016311c81919b1c2b07fbf3a8b9e80c86b905b8c9d96b53066784a452"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-step-model/README.md
    id: source.57796f0f7ef6
    hash: "blake3:ee52f2abd635191406a11140ffdffcad6852b9c9b95bd772526b230ad9b7d357"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
  - uri: impulse-step-model/src/lib.rs
    id: source.278ba5b51f6b
    hash: "blake3:fc182154f9dc96c73091cff566f657866aace20f15c798cd3546c665d60e18f0"
    retrieved_at: "2026-08-29T03:23:15Z"
    permission: public
claims:
  - id: claim.4047df10f81b
    claim_kind: extracted
    confidence: high
    cite: "impulse-step-model/README.md:3"
    source: source.57796f0f7ef6
    extract: extract.4047df10f81b
  - id: claim.61c574c56bbe
    claim_kind: extracted
    confidence: high
    cite: "impulse-step-model/src/lib.rs:59-69"
    source: source.278ba5b51f6b
    extract: extract.a6cff582fab7
  - id: claim.a111558df000
    claim_kind: extracted
    confidence: high
    cite: "impulse-step-model/src/lib.rs:71-81"
    source: source.278ba5b51f6b
    extract: extract.ab9aeb02aa34
  - id: claim.0e229793b3fd
    claim_kind: extracted
    confidence: high
    cite: "impulse-step-model/src/lib.rs:106-132"
    source: source.278ba5b51f6b
    extract: extract.c28c6bdea045
extracts:
  - id: extract.4047df10f81b
    text: "Pure, provider-neutral per-step model policy owned by the Impulse harness."
    text_hash: "sha256:bf53a9a1c3f281ba73654dcccb839c7716fec47cfa4d62d2bcbe5855f7c155e2"
  - id: extract.a6cff582fab7
    text: Default API worker context with no review or verification signal.
    text_hash: "sha256:d977dfbbfca78fa3b9515ffdca6cee7f8c9985597e4cf9ed9a947a6f6c9240a4"
  - id: extract.ab9aeb02aa34
    text: Default API supervisor context with no review or verification signal.
    text_hash: "sha256:46aaa07e56257e6b62aa2760c5cf9b9150fe2dbf78505508d7eb8a1aeb20e332"
  - id: extract.c28c6bdea045
    text: Choose the model for one harness step.
    text_hash: "sha256:2bd8bc8bcf194c4e257ffbf3c2dfd8063ee6918b6c6ff5884ad9de9148d18d05"
---

# impulse-step-model

Pure, provider-neutral per-step model policy owned by the Impulse harness. (impulse-step-model/README.md:3)

## lib.rs

`worker` — Default API worker context with no review or verification signal. (impulse-step-model/src/lib.rs:59-69)
`supervisor` — Default API supervisor context with no review or verification signal. (impulse-step-model/src/lib.rs:71-81)
`decide_step_model` — Choose the model for one harness step. (impulse-step-model/src/lib.rs:106-132)

## Sources

- [impulse-step-model/Cargo.toml](../../impulse-step-model/Cargo.toml)
- [impulse-step-model/README.md](../../impulse-step-model/README.md)
- [impulse-step-model/src/lib.rs](../../impulse-step-model/src/lib.rs)

## Symbols

- `function` `worker`
- `function` `supervisor`
- `function` `decide_step_model`
