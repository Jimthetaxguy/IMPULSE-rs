# AI Coding Impulse Spec-Driven Development Plan v1.1 Research Review

## Canonical inputs and spec reconciliation

The baseline requirements and technical constraints for the impulse originate in your PRD v1.0 JSON and its metadata (project name, owner, date, and doc type). fileciteturn0file0 fileciteturn0file1

Your v1.1 plan materially changes the early delivery profile by inserting **Phase 1.5 “Live Session Coordination Harness (SWARM)”** inside Phase 1, with an explicit MVP focus on same-repo, intra-session coordination across multiple concurrent agents in one Zellij workspace, mediated by `LIVE.md` and a lightweight local DB. That insertion is directionally consistent with the PRD’s emphasis on multi-project workflow excellence and persistent context, but the v1.1 version narrows the first “wow moment” to **multi-agent coordination** rather than **multi-project switching**. fileciteturn0file1

Two spec deltas are large enough to warrant explicit reconciliation before execution:

- **Resource footprint target**: PRD v1.0 sets a target of **<400MB memory** and **<80MB binary size** for a lightweight VS Code alternative. fileciteturn0file1 The v1.1 plan targets **<110MB total idle RAM** with a harness delta of **≤25MB**. This is achievable only if “idle” is defined tightly (no models loaded, no indexing jobs running, no heavy daemon memory included), and if you treat LLM inference servers as outside the idle budget or ensure they are not resident. This matters because local inference and embedding services can dominate RAM even when “idle,” unless explicitly stopped or configured. citeturn19view0

- **Local-first constraint vs heterogeneous agents**: PRD v1.0 supports both local and remote model providers by design (it explicitly supports “any LLM provider” through configuration, including local models). citeturn9search1 The v1.1 plan says “no network calls by default,” while also naming agents like Claude Code and others that normally call remote APIs unless proxied through local-compatible endpoints. This is solvable, but it needs a crisp boundary: “impulse telemetry + state is local by default” vs “LLM calls are local only in MVP.” citeturn19view0turn9search1

## Research-backed rationale for prune-first micro-compaction

Your v1.1 plan’s micro-compaction posture (continuous pruning and bounded injection, rather than emergency summarization at the cliff) is strongly supported by recent empirical findings on agent context management.

JetBrains Research reports that **observation masking** (selectively omitting older tool observations) outperforms LLM summarization in overall efficiency and reliability in their experiments, which is a direct endorsement of “prune-first” as the default context hygiene strategy. citeturn10search2 Their “Complexity Trap” work similarly argues that masking can match summarization outcomes while halving cost relative to a raw agent, reinforcing the design choice to treat summarization as a bounded tool rather than the primary mechanism. citeturn10search6turn10search22

OpenCode’s own product design converges on the same principle: its config exposes `compaction.prune` explicitly as “remove old tool outputs to save tokens,” alongside `compaction.reserved` as a buffer to avoid overflow during compaction. citeturn5view0 That alignment matters because Phase 1.5 wants to create multi-agent benefits without compaction disruption, and prune-first strategies produce benefits earlier, with less risk of meaning loss than aggressive summarization. citeturn10search2turn5view0

A concrete proof point that this approach is viable in practice is the emergence of third-party dynamic pruning extensions for OpenCode that implement view-layer pruning without rewriting history. The Dynamic Context Pruning plugin documents multiple “zero LLM cost” strategies (deduplication, superseding writes, purging errored tool inputs), and explicitly states that session history is never modified and pruned content is replaced with placeholders only in what gets sent to the model. citeturn11view0 This is effectively the “steward as governance of the view-layer” pattern you want to operationalize.

## Execution feasibility via OpenCode and Zellij integration seams

Phase 1.5 hinges on two core integration surfaces: the agent runtime layer (hooks/events) and the workspace UI layer (lightweight visibility controls). The current public APIs for both are strong enough to implement your harness, with two important corrections to the event vocabulary.

On the OpenCode side, the plugin system supports TypeScript plugins, automatic plugin loading from `.opencode/plugins/`, and optional dependency installation via Bun at startup. citeturn2view1turn6search2 The events and hooks you reference in v1.1 exist, with slight naming differences:

- Session events include `session.idle`, `session.status`, `session.updated`, and `session.compacted`. citeturn3view0  
- Message events include `message.updated`. citeturn3view0  
- Tool lifecycle hooks are `tool.execute.before` and `tool.execute.after` (not `tool.executed`). citeturn3view0  
- TUI hooks include `tui.prompt.append` and `tui.toast.show`, which are well-matched to “stealth by default” (no user-visible output unless toggled) if you avoid toast unless configured. citeturn3view0  

For compaction-specific governance, OpenCode documents `experimental.session.compacting`, a hook that fires before generating the continuation summary and allows either context injection (`output.context.push(...)`) or full prompt replacement (`output.prompt = ...`). citeturn2view0 That is an ideal insertion point for your “micro-compaction artifacts should survive compaction” requirement.

For **continuous agent-to-agent injection**, however, the compaction hook alone is not sufficient, because it is only called at compaction time. The more direct mechanism is to inject into outbound model requests. OpenCode has experimental transform hooks in the wild, notably `experimental.chat.system.transform` and `experimental.chat.messages.transform`. Their existence is evidenced in OpenCode’s own issue discussion describing how the `experimental.chat.system.transform` plugin trigger is invoked in core session code. citeturn16view0 The dynamic pruning plugin uses these hooks to transform the system prompt and message list immediately before requests are sent, which is exactly the “view-layer governance” mechanism your SWARM harness needs for bounded `[SWARM]` injection. citeturn14view0turn11view0

On the Zellij side, your UI plan is compatible with current capabilities:

- Zellij supports floating panes that can be pinned “always on top,” and the plugin command `set_floating_pane_pinned` implements that behavior (it requires `ChangeApplicationState`). citeturn0search2turn6search3  
- Zellij’s plugin system includes a permissions model and requires plugins to request permissions via `request_permission`. citeturn6search0  
- Zellij’s event model includes `PaneUpdate` and `SessionUpdate`, which can support a thin status view that is purely observational (agent count, last injection timestamp) rather than invasive. citeturn6search15  

image_group{"layout":"carousel","aspect_ratio":"16:9","query":["Zellij pinned floating panes terminal screenshot","OpenCode terminal AI coding assistant interface","Ghostty terminal emulator macOS screenshot"],"num_per_query":1}

A crucial stealth-mode implication: Zellij plugin permission prompts are inherently user-visible at least once. If “zero user-visible prompts” is interpreted literally, Phase 1.5 should implement stealth mode first via OpenCode plugin only, and treat the Zellij status pane as optional once permissions are granted. citeturn6search0turn0search2

## LIVE.md and live_state.db design, with evidence-based model and storage choices

Your v1.1 plan uses two stores in Phase 1.5:

- `LIVE.md` as a human-auditable, git-ignored scratchpad in the project root.
- `.impulse/live_state.db` as machine-readable state, with a vector table for patterns and a table tracking active agents.

This is a reasonable “dual-plane” design (human cache + machine index), and it aligns with later PRD phases that add RAG and extracted memory. fileciteturn0file1 The key research-driven refinements are about vector storage maturity and embedding model selection.

**Vector storage:** `sqlite-vec` is a credible choice for a portable local vector store. It is explicitly designed as a small “fast enough” SQLite extension that runs where SQLite runs, supports storing and querying vectors in `vec0` virtual tables, and is written in C with no dependencies. citeturn1search0 The trade-off is that it is described as “pre-v1” with breaking changes expected. citeturn1search0 For Phase 1.5, that implies you should keep the live DB schema minimal and versioned, and avoid deep coupling to advanced features until Phase 2.

**Embeddings:** The spec proposes using “Qwen3-0.6B (Ollama) embed” for cosine similarity. The critical nuance is that Qwen3-0.6B is a general LLM (a small model in a broader family), not a dedicated embedding encoder. citeturn1search7turn1search3 If you want robust similarity at low latency, you should use a dedicated embedding model through Ollama. Ollama’s `nomic-embed-text` is explicitly an embedding-only model “used to generate embeddings,” and it requires an Ollama version gate. citeturn0search3turn0search7 This supports a clean Phase 1.5 split:

- Use an embedding model (for example, nomic-embed-text) for similarity and clustering.
- Use a lightweight “small model” or rules-based heuristics for summarizing and formatting the `[SWARM]` snippet.

**Local provider plumbing:** You can keep OpenCode local by configuring an OpenAI-compatible provider pointing to `http://localhost:11434/v1`, as shown in Ollama’s OpenCode integration docs. citeturn19view0 Those docs also note that OpenCode expects larger contexts, recommending at least a 64k context window, which is relevant to how quickly you would hit compaction and how much buffer `compaction.reserved` should keep in practice. citeturn19view0turn5view0

Finally, your plan’s decision to cap injection at 120 tokens becomes easier to enforce if SWARM output is treated as a structured message fragment and injected through a transform hook, not as raw chat output. OpenCode’s plugin model already supports modifying outbound behavior through hooks and pre/post tool interception. citeturn3view0turn3view3

## Interop reality check for “4–12 agents” and “zero coaching” across heterogeneous CLIs

Your vision explicitly includes multiple agent CLIs (OpenCode, Claude Code, Aider, and others) coordinating inside the same repo. The research and current tool behaviors suggest an important split between what is feasible in Phase 1.5 without wrappers and what is not.

**Within OpenCode:** Multi-agent composition is first-class. OpenCode supports primary agents and subagents, can switch agents during a session, and includes hidden system agents for compaction/summaries. citeturn7view0 This makes it the most realistic initial target for “silent coordination with no user coaching,” because plugins can observe tool usage, message updates, session state transitions, and inject into the prompt pipeline. citeturn3view0turn2view0turn16view0

**Aider:** Aider’s model is “files are added to the chat session,” and users often need to explicitly manage which files are in context. citeturn17search0turn17search8 A standing limitation is that changes made outside Aider may not be recognized unless the user restarts or removes and re-adds files, according to user reports and issue discussion. citeturn17search4 That makes “LIVE.md updates automatically influence all Aider agents with zero coaching” difficult unless you build a wrapper that forces periodic `/read LIVE.md` or restarts sessions, which would violate stealth.

**Claude Code:** Claude Code’s documented “memory” mechanism loads the first 200 lines of `MEMORY.md` at session start, and topic files are loaded on demand. citeturn9search2turn17search9 But multiple discussions and issues request hot reload behavior for configuration files like `CLAUDE.md`, because running sessions do not reliably pick up changes without restarting. citeturn17search17turn17search2 This means a constantly updated `LIVE.md` will not necessarily be ingested automatically by Claude Code sessions, absent a wrapper or an explicit “read this file now” behavior.

**Implication for v1.1 success metrics:** If the MVP acceptance criteria require “4+ agents detect overlap and inject within 30s with zero user-visible interaction,” the most research-consistent interpretation for Phase 1.5 is: **4 OpenCode agents/sessions in one repo**, with SWARM injection implemented via OpenCode plugin transforms. citeturn3view0turn16view0turn7view0 Heterogeneous agent support can remain a Phase 2+ extension, delivered through wrappers or an MCP-style sidecar tool that other agents can call when they support it.

A minimal, execution-safe support matrix for Phase 1.5 is below.

| Agent CLI | Phase 1.5 support | Mechanism |
|---|---|---|
| OpenCode | Full | Plugin transforms |
| Claude Code | Limited | Manual read / restart |
| Aider | Limited | Manual /read |

The table reflects how these tools work today, not the long-term goal. citeturn7view0turn17search4turn17search17

## Testing strategy aligned to context retrieval research and Phase 1.5 acceptance criteria

Phase 1.5 has unusually crisp operational acceptance criteria (latency, token budget, RAM budget, stealth behavior). The most robust testing approach combines deterministic harness tests with process-oriented context evaluation research.

**Coordination correctness and latency:** You can test “inject within 30s” deterministically by scripting four OpenCode sessions (or panes) that execute a known overlap pattern (same file reads/edits, same tool signatures), then verifying that the injection log entry appears in the target session within 30 seconds. The OpenCode plugin system already provides structured logging via the SDK client and a reliable event stream for `message.updated` and session state. citeturn3view0turn2view0

**Token-budget compliance:** Enforce the ≤120-token injection requirement using a strict formatter plus truncation rule, and validate with a unit test that takes worst-case pattern text and produces a bounded output. The existence of `tui.prompt.append` plus message/session hooks supports validation that the injected string and provenance prefix are always present in the same shape. citeturn3view0turn2view0

**Micro-compaction safety:** Base Phase 1.5 pruning on observation masking and deterministic strategies first, in line with evidence that masking can outperform summarization in agent efficiency and reliability. citeturn10search2turn10search6turn10search22 If you use summarization at all in Phase 1.5, constrain it to “tool-output compression” rather than “decision compression,” mirroring strategies used by pruning plugins that keep the session history intact but reduce what is sent. citeturn11view0turn14view0

**Evaluation framework for later phases:** When you transition from live-pattern injection to a full context retrieval layer (Phase 2), ContextBench is a direct fit for measuring whether your retrieval and injection improve context recall/precision in coding-agent trajectories. It is explicitly designed as a process-oriented benchmark for coding agents, with human-annotated gold contexts and metrics tracking context recall, precision, and efficiency during issue resolution. citeturn10search1turn10search9turn10search21 In parallel, LoCoMo provides a strong memory benchmark for your longer-horizon extraction layer, with very long-term, multi-session conversational data and tasks assessing long-range temporal and causal memory. citeturn10search0turn10search8

**RAM budget verification:** The only credible way to validate “≤25MB harness delta” is measurement. Your plan to use `ps`-style measurement is sound, but be explicit about what you count as harness (plugin runtime, SQLite process, embedding calls) vs what you count as part of OpenCode or Ollama. OpenCode and Ollama integration documentation confirms that local model serving is a distinct provider configuration, which can be separated for measurement purposes. citeturn19view0turn2view1

**Licensing and reuse risk:** If you plan to reuse dynamic context pruning code, note that prominent pruning plugins are AGPL-3.0 licensed. That is compatible with some distributions but can impose strong copyleft obligations if incorporated into your own distributed codebase, so Phase 1.5 should either (a) remain independent and not copy AGPL code, or (b) treat such components as optional external plugins rather than merged code. citeturn11view0