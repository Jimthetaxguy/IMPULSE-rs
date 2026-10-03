---
title: "ADR-0022: Typed Model Endpoints (Protocol, Not Vendor)"
description: Model endpoints are typed values (protocol, base URL, auth by env name, model, output limit) in named config profiles selected per role, so Ion and photon can use any model
status: review
created: 2026-10-03
updated: 2026-10-03
type: decision
category: architecture
phase: all
audience: builders
deciders: [Impulse Maintainers]
tags: [adr, llm, providers, ion, photon, configuration]
---

# ADR-0022: Typed Model Endpoints (Protocol, Not Vendor)

## Status

Proposed on lane `claude/ion-scout-subagent-20261002`. Stage 1 (types, config schema, photon
resolution) is implemented there. Stage 2 (Ion's own provider path) waits for operator review of
this ADR.

## Context

Ion selects a model through `ImpulseProvider`, a closed enum of three vendors (Anthropic, OpenAI,
MiniMax). Each vendor has a hard-coded request path; the `*_BASE_URL` overrides change only the
origin (ADR-0015 context), so an OpenAI-compatible endpoint under another path, such as OpenRouter's
`/api/v1`, cannot be reached. Only the Anthropic provider populates tool calls, so Ion can chat on
other vendors but cannot use its tools there. Selection comes from `IMPULSE_PROVIDER` and
`IMPULSE_MODEL`.

The photon subagent (`docs/superpowers/specs/2026-10-02-ion-photon-subagent-design.md`) wraps
`min-agent`, which already speaks three wire protocols with conformance fixtures, typed provider
failures, bounded retry, and native item replay. Its first version hard-coded Anthropic Messages.

James asked that photon, and Ion itself, run on any model endpoint, with the configuration typed and
modular.

## Decision

1. **An endpoint is a protocol, not a vendor.** `model_endpoint::ModelEndpoint` holds a
   `WireProtocol` (`anthropic_messages`, `openai_chat`, `openai_responses`), a full base URL with its
   path, an `EndpointAuth` (`none`, `bearer_env { env }`, `header_env { header, env }`), a model id,
   and an optional output-token limit. OpenRouter, LiteLLM, Ollama, vLLM, MiniMax's OpenAI-compatible
   API, OpenAI, and Anthropic are all just endpoints.
2. **Validation happens once, at the boundary.** A typed `EndpointError` rejects blank models,
   unparsable or credential-bearing URLs, plain HTTP to anything other than numeric loopback, invalid
   env or header names, and an Anthropic Messages endpoint without an output-token limit.
3. **Credentials are referenced, never stored.** Auth names an environment variable. Config never
   holds a secret, and because rule 2 rejects credential-bearing URLs, an endpoint value is safe to
   log.
4. **Named profiles, selected per role.** `config.json` gains `model_endpoints: { profiles: {name:
   ModelEndpoint}, roles: { ion?, photon? } }`. A role names a profile; a missing profile is an error,
   never a silent fallback. The section is part of `Config`, so `config set` round-trips it.
5. **Host-owned selection (ADR-0015 unchanged).** The harness resolves the endpoint. No tool schema
   exposes a model or endpoint choice. `decide_step_model` still chooses only the model id for a
   step, within the resolved endpoint.
6. **Resolution order for photon:** the `roles.photon` profile; otherwise Anthropic Messages at Ion's
   own Anthropic origin with `ANTHROPIC_API_KEY`, model `ION_PHOTON_MODEL` or the default. When
   `IMPULSE_PROVIDER` names a non-Anthropic provider and no photon profile exists, resolution fails
   with a message naming the config section instead of guessing a model for that vendor.
7. **Ion (stage 2):** Ion resolves `roles.ion` the same way. Legacy `IMPULSE_PROVIDER` /
   `IMPULSE_MODEL` are translated into an equivalent endpoint, so existing setups keep working.
   Non-Anthropic protocols reach Ion through `min-agent`'s adapters behind an `LlmProvider`
   implementation, so the binary has one tested wire layer for those protocols; the native Anthropic
   provider stays for prompt caching.

## Consequences

- Photon runs on any of the three protocols today. Ion keeps its current provider path until stage 2.
- The `model_endpoint` types are pure and serde-typed; only `model_endpoint::min_agent_bridge`
  (behind `photon-subagent`) knows `min-agent`'s config shapes.
- Stage 2 changes Ion's live provider path and needs its own review: `min-agent`'s client is
  blocking and must run on `spawn_blocking`, and Ion's `Message` history must map to `min-agent`'s
  native `Item` replay without losing tool-call ids.
- No capability negotiation is implied. A profile that points at a model without tool support fails
  at the first tool turn with a typed provider error.

## Alternatives Considered

- **Add vendors to `ImpulseProvider`.** Keeps the vendor coupling and the origin-only override, and
  every new gateway needs code.
- **Extend Ion's own OpenAI and MiniMax providers with tool calls.** Duplicates `min-agent`'s three
  adapters and leaves two wire layers to maintain.
- **Delegate selection to a gateway (LiteLLM, OpenRouter).** Rejected by ADR-0015: per-step model
  choice stays harness-owned. A gateway remains usable as one endpoint.
