# impulse-step-model

Pure, provider-neutral per-step model policy owned by the Impulse harness.

The crate decides only between a caller-resolved current/configured model and
an optional caller-admitted escalation model. It does not decide whether an
LLM runs, select a provider, perform HTTP, inspect prices or token counters,
load configuration, or write audit state.

Hosts retain four responsibilities:

1. decide whether inference is allowed for the operation;
2. resolve a non-empty provider-compatible model candidate;
3. adapt native actor and verification facts into `StepModelContext`; and
4. record the returned `StepModelDecision` in the host's own evidence system.

This boundary lets ROSA and other applications reuse the policy without
depending on Impulse's TUI, PTY, SQLite, office, credential, or HTTP graph.
