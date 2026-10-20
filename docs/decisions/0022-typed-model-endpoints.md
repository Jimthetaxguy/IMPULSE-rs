---
title: "ADR-0022: Typed Model Endpoints (Protocol, Not Vendor)"
description: Model endpoints are typed values (protocol, base URL, auth by env name, model, output limit) in named config profiles selected per role, so Ion and photon can use any model
status: review
created: 2026-10-03
updated: 2026-10-04
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
resolution) is implemented there. Stage 2 (a provider trait and Ion on typed endpoints) is
implemented on `claude/model-provider-20261004`; see [Stage 2 amendment](#stage-2-amendment-2026-10-04).
It replaces the min-agent route that rule 7 originally named.

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
3. **Credentials are referenced, never stored, and go only where the user allows.** Auth names an
   environment variable. Config never holds a secret, and because rule 2 rejects credential-bearing
   URLs, an endpoint value is safe to log (validation errors do not echo a URL that fails to parse).
   `config.json` is project data that a cloned repository controls, so a profile from it may send its
   credential only to the protocol's vendor host (`api.anthropic.com`, `api.openai.com`), a numeric
   loopback address, or a host the user lists in `IMPULSE_TRUSTED_MODEL_HOSTS` in their own
   environment. Anything else is refused with a message naming that variable
   (`EndpointError::UntrustedCredentialHost`). Endpoints Ion builds from the user's environment
   (`ANTHROPIC_BASE_URL`) are not subject to this rule.
4. **Named profiles, selected per role.** `config.json` gains `model_endpoints: { profiles: {name:
   ModelEndpoint}, roles: { ion?, photon? } }`. A role names a profile; a missing profile is an error,
   never a silent fallback. `Config` keeps the section as raw JSON, so `config set` round-trips it and
   a malformed section never stops the rest of the configuration (and every command) from loading;
   its consumers parse it and report errors when they use it.
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

## Stage 2 amendment (2026-10-04)

James directed stage 2 to a native provider trait, informed by a study of LangChain's provider
abstraction (`langchain-provider-abstraction-research.md`): adopt its small provider contract,
unified tool-call types, and capability metadata; drop its Runnable layer, model-name inference,
`with_structured_output`, and separate sync/async paths; and merge its three retry, fallback, and
routing mechanisms into one. This supersedes rule 7's plan to reach non-Anthropic protocols
through `min-agent`, whose client is blocking. Photon keeps using `min-agent`.

8. **One small trait.** `model_provider::ModelProvider` has `capabilities()`, `generate()`, and
   `stream()`. `stream()` has a default that replays `generate()`, so every provider can stream.
   Providers are `Arc`-shareable and hold no conversation state.
9. **Capabilities are data.** `ModelCapabilities` (tool calling, vision, streaming, structured
   output, context window) defaults from the wire protocol and is narrowed per profile with
   `capabilities` in `config.json`. A request needing tools or images is refused before any network
   call by an endpoint that does not declare them. Nothing is inferred from a model id.
10. **One neutral message model.** `ContentBlock::{Text, Image, ToolCall, ToolResult}` normalizes
    Anthropic `tool_use` blocks with object `input` and OpenAI `tool_calls` with string
    `arguments` (and Ollama's occasional object). Arguments that are not a JSON object become an
    `InvalidToolCall`, never an error and never a guessed value. One `StreamAccumulator` merges
    stream fragments for every provider.
11. **Two wire providers.** `AnthropicProvider` (Messages) and `OpenAiChatProvider` (Chat
    Completions: OpenAI, Ollama, OpenRouter, vLLM, LiteLLM), each with non-streaming and SSE paths.
    `openai_responses` is refused by name at build time until it has a native provider; photon
    still reaches it through `min-agent`. Structured output is validated by the harness, not
    offered as a model method. `provider_params` passes vendor fields through unmodeled but can
    never replace the keys the harness owns (`model`, `messages`, `system`, `tools`, `stream`,
    `stream_options`, `max_tokens`). Redirects are refused, so a credential header is never sent
    to another host, and a stream that is silent for 120 seconds fails with a timeout.
12. **One endpoint policy.** `model_endpoints.policy` (`routing`: `local_first` by default or
    `in_order`; `retry`: attempts and capped exponential backoff) drives `PolicyProvider`, which is
    itself a `ModelProvider`. One error classification decides everything: transport, timeout,
    408, 429, and 5xx retry the same endpoint; a `Retry-After` longer than the backoff cap moves on
    instead of retrying early. Every other failure moves to the next endpoint, including 400 and
    422: a local model's "does not support tools" or "context too long" is a 400 that a different
    model behind the next endpoint may not return. A body that is not JSON is `InvalidResponse`
    and moves on. A stream falls back only before its first event.
13. **Local first.** An endpoint on a numeric loopback address (the same test rule 2 uses for plain
    HTTP) is tried before remote ones, keeping configured order within each group.
14. **Roles.** `orchestrator` joins `ion` and `photon`. `model_endpoints.fallbacks.<role>` lists
    further profiles; an unknown fallback is a validation error, consistent with rule 4. The router
    maps a role to a `PolicyProvider` over its own profile plus fallbacks. Which model a role runs
    is configuration, never a built-in quality tier.
15. **Composition through tower.** `ProviderService` makes any provider a
    `tower::Service<ModelRequest>`, so timeouts and concurrency limits come from tower layers
    instead of a bespoke chain abstraction.
16. **Ion on endpoints.** When `model_endpoints` assigns Ion a profile or fallbacks, `ion` builds
    its provider from the router (`ChatState::from_config_or_env`) and runs its unchanged tool loop
    on it through `ion_bridge::ModelProviderLlm`. Only Ion's own profiles, fallbacks, and the
    policy are validated for Ion; a broken Ion section fails closed at the first turn
    (`ProviderSelectionError::InvalidEndpointConfig`). Without an Ion assignment, including when
    `config.json` is unreadable or another role's profile is broken, Ion's provider is exactly the
    legacy `IMPULSE_PROVIDER` one. On this path the endpoint governs the output limit and sampling
    (Ion's built-in 4,096-token and 0.7 defaults are not sent), tool calls reach the loop as
    `ToolUse` even when a server reports `stop`, and a malformed tool call fails the turn. The
    endpoint names the model, so the step-model hook's per-step model id is not applied on this
    path yet.
17. **Credential destinations hold for every role, and photon follows the configuration.** Rule
    3's vendor-host, loopback, or `IMPULSE_TRUSTED_MODEL_HOSTS` check runs inside `validate_role`,
    so Ion's own profiles and fallbacks obey it, not only `validate`. When Ion is on typed
    endpoints, or photon has fallbacks but no `roles.photon`, photon refuses rule 6's Anthropic
    default and asks for `model_endpoints.roles.photon` instead of sending the question and file
    excerpts to a vendor the user did not choose. A `model_endpoints` section that does not parse
    and does not name Ion leaves Ion on the legacy path with a printed note.

**Review.** An adversarial pass (2026-10-04, reproductions in a scratch copy) found three P1s, a
tool call dropped on `finish_reason: "stop"`, a local 400 that stopped the fallback chain, and an
unrelated broken profile that disabled Ion, plus P2s and P3s on output limits, stream stalls,
unindexed tool-call deltas, retry classification, and `provider_params`. All are fixed with
regression tests; the rules above describe the fixed behavior.

**Evidence.** Unit tests cover wire mapping in both directions, SSE parsing, accumulation,
every policy branch (under paused time), routing order, config validation, and the Ion bridge.
Loopback HTTP tests run both providers through `reqwest` and the SSE reader against recorded
response bytes, including 429 `Retry-After`, 401, and fallback from a dead local port. Opt-in
real-system tests (`IMPULSE_OLLAMA_IT=1`, `--ignored`) passed against a local Ollama with
`qwen3:8b` and `llama3.1:8b` on 2026-10-04: generate, stream, a `file_read` tool call, and one Ion
turn driven through `ChatState::from_config_or_env`.

**Not in stage 2.** A native `openai_responses` provider; per-step model choice across endpoints;
cost tracking and token budgets; latency- or quality-threshold routing; and moving photon off
`min-agent`.

