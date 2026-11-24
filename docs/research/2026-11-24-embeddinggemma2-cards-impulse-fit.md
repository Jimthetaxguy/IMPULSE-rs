# Fit check: EmbeddingGemma 2 whole-tool-card shortlist

Handoff: 2026-11-24-grokky-impulse-embeddinggemma2-cards (from Grokky). Row: embeddinggemma2-tool-clusters-2026-11-21 in research-ai-patterns synthesis/register/rows (branch research/2026-11-24-daily). AIden's numbers (167 cards, 30 cases, full card 23/30 top-1 and 27/30 top-3, MCP tools 4-5 of 12) were not re-checked here; the row says n=30 from a single author.

## Decision: partial fit, for the Ion tool registry, not the picker

The picker is unaffected: `decide_step_model` names a model and never sees tools. Tool exposure lives elsewhere. `impulse-rs/src/ion_repl/chat.rs` builds `tool_definitions(registry)` and sends every registered tool on each round; nothing shortlists them. Impulse already has a retrieval index with optional vectors (`impulse-rs/src/retrieval`, `handlers/retrieval.rs`), but it indexes history and genome entries, not tools.

A shortlist only pays off once the tool list is large enough to crowd the context or confuse selection, which mostly happens when many MCP tools are loaded. The cheaper lesson from the row applies today: write fuller tool descriptions, since MCP tool cards scored worst and card text mattered more than vector tricks.

## Held for Jim

No code change. If wanted later: (1) improve tool description text in the registry, then (2) an Ion-side shortlist that embeds whole tool cards through the existing retrieval index, only when the tool count passes a threshold. No MaxSim clusters. Read from /workspace/merge-all/IMPULSE-rs at 2ba676d (Oct 19), so likely, not verified against current main.
