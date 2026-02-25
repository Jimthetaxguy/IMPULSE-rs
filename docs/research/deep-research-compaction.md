# Agentic Compacting for CLI Coding Agents

## Compaction today and what it actually does

“Compaction” in agent systems is usually a reactive intervention: when a conversation nears a model’s context limit, the system summarizes prior turns and restarts a fresh context window using that summary. citeturn7view0turn17search2 This can preserve continuity, but it also changes the information substrate the agent is reasoning over, which is why it can feel like the agent “became a different coworker” mid-task.

On the entity["company","Anthropic","ai lab | san francisco, ca"] platform, automatic compaction for tool-driven workflows is implemented as a threshold-triggered loop: token usage is tracked, and once a threshold is exceeded the system pauses before the next call, asks for a summary, replaces the entire message history with that summary, and then continues. citeturn25view0turn5view1 The cookbook explicitly supports choosing a different model for summarization than the main task model, along with configurable thresholds and custom summary prompts. citeturn5view3turn25view2 This “different model for summarization” option is a direct precedent for the architecture you described: a separate model doing context management work while the main coding agents stay focused. citeturn25view2turn13search0

Claude Code similarly exposes levers to influence what survives compaction: you can add “Compact Instructions” in `CLAUDE.md` or run `/compact` with an explicit focus. It also surfaces introspection for context costs, including a reminder that MCP server tool definitions consume context and can be audited. citeturn3view3turn3view10

In entity["company","OpenCode","ai coding agent project"], compaction is not only a summary step. There is an explicit “prune” feature intended to remove older tool outputs to save tokens, controlled via config (`compaction.auto`, `compaction.prune`, `compaction.reserved`). citeturn4view4turn3view2 Beneath the hood, OpenCode has a built-in hidden “compaction” agent that compacts long context into a smaller summary and runs automatically (not selectable in the UI). citeturn22view3 That is already close to your “control agent” idea, but it is primarily reactive (kicks in when overflow is imminent), not continuously curatorial.

Finally, entity["organization","OpenHands","ai agent framework"] frames the same challenge as “conversation history compression” and formalizes it as a Condenser system with responsibilities like threshold detection, summary generation (LLM or heuristics), and transforming history into an LLM-ready “view.” citeturn4view5turn3view4 It also documents composable condenser types like a rolling window condenser and a pipeline condenser, which is essentially a production-ready acknowledgment that context compression is best treated as a modular chain rather than a monolith. citeturn4view5

## What “agentic compacting” adds beyond standard compaction

Your idea can be described precisely as a **Context Steward sidecar**: a lightweight, intermittently active agent that monitors ongoing sessions (possibly across multiple CLIs for the same repo), continuously refines what the primary coding agents see, and writes durable artifacts so the system needs fewer disruptive full compactions.

The core shift is **reactive compaction → proactive context governance**:

- Reactive compaction: summarize only when you are near the limit; accept that the summary is a lossy “single shot.” citeturn7view0turn24view1  
- Agentic compacting: perform frequent micro-operations that keep the working set small and high-signal, so you delay or even avoid full compaction, and when full compaction does occur it is operating over already curated state. citeturn23view1turn25view0

The strongest evidence that “micro-operations first” is the right mental model comes from empirical work on agent context management. entity["company","JetBrains Research","research group | prague, cz"] reports that simple “observation masking” (omitting older observations/tool outputs) can outperform LLM summarization on efficiency and reliability in their experiments, and that LLM summarization tends to elongate agent trajectories (they cite ~15% longer runs in their analysis for some model configurations). citeturn0search2turn7view6turn1search1 The same line of work explicitly finds that a combined approach (masking plus summarization) can deliver additional cost reductions relative to either alone, indicating that “hybrids win” when carefully designed. citeturn16view2turn16view1

This matters because your micro-compaction agent, if it is too summary-heavy, can accidentally make the downstream coding agents *less* efficient by smoothing over “stop signals” and encouraging extra exploration. citeturn7view6turn1search1 So the research points toward a stewardship strategy that is mostly deterministic pruning/masking, with targeted summarization as the exception rather than the default.

## Architecture patterns that make this feasible in a CLI-first “impulse”

Your notes already lean toward a multi-tier memory hierarchy. The key refinement is to treat “micro-compaction” as a **view-layer problem** plus a **persistence-layer problem**, not as destructive rewriting of chat history.

### Event-driven steward with explicit wake signals

If you are using OpenCode as the integration surface, plugins are a practical place to attach a steward. Plugins can subscribe to rich events including `message.updated`, `session.status`, `session.idle`, `session.compacted`, and also TUI hooks like `tui.prompt.append`. citeturn23view1turn23view3 That means your steward can be dormant most of the time and “wake” only on meaningful signals:

- burst of tool output
- a new user prompt being composed
- session becomes idle
- context usage crosses soft thresholds
- compaction is about to occur, or just occurred

OpenCode also provides a dedicated compaction hook: `experimental.session.compacting` fires before the model generates the continuation summary, and you can inject additional context or even replace the compaction prompt. citeturn3view1turn4view2 In the OpenCode codebase, that hook is invoked explicitly during compaction processing, and the system publishes `session.compacted` events, giving you a stable seam for integrating a steward. citeturn20view0

If you are using MCP servers for sidecar services, MCP’s protocol supports notifications as a core message type (in addition to bidirectional requests and responses) and standard transports like stdio and Streamable HTTP. citeturn3view8turn3view9 That lets you implement a “watcher” process that can react to events without constant polling, depending on the host’s capabilities.

### Micro-compaction primitives you can chain

A steward that does “smart micro compacting” should be built from primitives, roughly in this order of precedence:

1. **Prune / mask low-value tool outputs** (deterministic and cheap)  
   OpenCode’s internal compaction logic demonstrates this pattern: it defines pruning thresholds (for example, it protects a recent window of tool parts and only prunes beyond that, and it only applies pruning if enough tokens would be saved). citeturn20view0turn4view4

2. **Extract durable state into a structured “working set”**  
   This is where your steward writes a small number of canonical blocks that are always injected:
   - goal and current status
   - active constraints and preferences
   - “current todo stack”
   - open bugs and last known failing commands
   - key decisions made during the session

   This design aligns with the fact that both Anthropic’s cookbook and OpenCode’s compaction prompt templates emphasize preserving goal, progress, relevant files, and next steps. citeturn25view0turn20view0

3. **Summarize only specific stale segments** (targeted, bounded, reversible)  
   Full summaries inherently lose information; the Anthropic cookbook is explicit about this trade-off and recommends custom prompts and higher thresholds when historical detail is critical. citeturn24view0turn7view0 The steward should apply summarization only to segments with low expected future value, and keep provenance so the raw text is recoverable.

4. **Build “context packs” for injection** (bounded and provenance-tracked)  
   Claude Code and OpenCode both warn that tool definitions and enabled servers cost context. citeturn3view3turn12view1 So a steward should build context packs with explicit token budgets and keep them small, rather than indiscriminately injecting everything it knows.

A concise way to operationalize “micro-compaction” is to define a **three-tier working set** that the steward maintains continuously:

| Working set layer | Contents | Primary mechanism |
|---|---|---|
| Hot | Last few turns, active files, latest errors, current plan | No compression |
| Warm | Prior resolved subthreads, long tool outputs, repeated file reads | Mask/prune first citeturn20view0turn16view2 |
| Cold | Completed exploratory threads, older brainstorms | Targeted summaries with provenance citeturn24view0turn7view6 |

### Separate-model stewardship is already a proven knob

Your intuition that the steward does not need to be the “smartest” model matches how compaction is described in the Anthropic SDK cookbook: you can route summarization to a cheaper and faster model than the primary workload. citeturn25view0turn5view3 OpenCode’s architecture similarly treats compaction as a distinct agent flow and explicitly triggers a compaction agent during processing. citeturn20view0turn22view3

The practical implication: you can design the steward as an **ephemeral worker** that loads only the delta since last wake, produces outputs, writes them to disk (or through an MCP server), and then discards its own context. This sidesteps long-lived-agent costs and reduces the risk of the steward itself “drifting.”

## Multi-CLI, multi-thread, same-repo: sharing context without bleeding across projects

The key to safe cross-session context sharing is **project-scoped persistence**. Claude Code already has a memory model that can serve as your persistence substrate:

- It distinguishes persistent “Auto memory” (Claude-authored notes) and explicit `CLAUDE.md` instructions (user-authored). citeturn11view0  
- Auto memory is stored per project under `~/.claude/projects/<project>/memory/`, and the project path is derived from the git repository root, meaning all subdirectories in the same repo share that memory directory by default. citeturn11view2  
- Only the first 200 lines of `MEMORY.md` are loaded at session start; more detailed topic files are loaded on demand. citeturn11view2  
- `CLAUDE.md` files are loaded in a directory hierarchy with precedence rules, and “child” memories are loaded on demand when files in those subtrees are accessed. citeturn11view4

This is almost exactly what your steward needs: a place to write small “index” style state and separate large details into topic files, while keeping session startup context bounded.

A strong, practical architecture is:

- **Within a repo:** steward maintains a shared “Project State” artifact (like `MEMORY.md` index + topic files) used across all simultaneously open CLIs. citeturn11view2  
- **Within a session:** steward maintains a session-local “Working Set” (hot/warm/cold) and uses view-layer masking/pruning to keep the active context small. citeturn16view2turn23view1  
- **Across repos:** default to isolation. Only allow cross-project memory when explicitly opted-in via “imports” or additional directories, because the official mechanism for pulling in external memory is gated and auditable. citeturn11view4

If you want a “steward agent” inside Claude Code itself rather than as an external app, subagents provide a native pattern for isolating high-volume operations: Claude Code’s docs explicitly describe subagents like Explore being used to keep exploration results out of the main conversation context. citeturn10view1 That is a direct analog to your idea: run the steward sidecar work in an isolated context, then inject only a bounded result.

## How to test agentic compacting so you can be confident it is truly better

Agentic compacting has two failure modes that are easy to miss if you only measure “did it finish the task”:

- **Silent relevance loss:** you pruned or summarized away the one detail that later becomes critical. citeturn24view0turn7view0  
- **False efficiency:** summaries reduce tokens but increase agent steps, making total time and cost worse. citeturn7view6turn1search1  

A robust evaluation plan needs both end-to-end success metrics and process-level context metrics.

### Use process-oriented benchmarks for context retrieval and consolidation

A recent benchmark specifically targets “context retrieval quality” in coding agents: ContextBench introduces gold contexts and evaluates recall, precision, and efficiency throughout issue resolution, not just patch success. citeturn8view0turn8view2 It includes 1,136 issue-resolution tasks across 66 repositories and eight programming languages, with human-annotated gold contexts at file, block, and line levels. citeturn8view0turn8view0 It also explicitly notes consolidation as a bottleneck: agents may inspect relevant code but fail to retain or use it in final patch generation. citeturn8view2

This is almost tailor-made for testing your steward:

- Hypothesis: micro-compaction improves “used context precision” without harming recall.
- Measurement: compare retrieved and injected context against gold context, and track whether injected items appear in the agent’s eventual patch reasoning and edits.

### Use agent efficiency research to guide thresholds and strategies

The JetBrains work and accompanying materials suggest:
- observation masking can reduce cost substantially without degrading downstream performance;  
- pure LLM summarization is not consistently superior;  
- a combined approach can yield additional measurable cost improvements. citeturn16view2turn16view1turn7view6  

So your tests should explicitly compare at least three strategies:

1. Baseline: no steward, default compaction.
2. Prune-first steward: deterministic masking/pruning plus structured working set.
3. Summary-heavy steward: frequent micro-summaries.

You should expect (3) to sometimes look good on token usage but regress on time-to-done or number of turns, consistent with “trajectory elongation” concerns. citeturn7view6turn1search1

### Combine end-to-end task success with observability metrics

For end-to-end correctness, SWE-bench Verified is a widely used human-validated evaluation subset for real-world software issues. citeturn1search10 Use it as a sanity check that your steward does not break the agent’s ability to patch repos and pass tests.

To ensure you can diagnose regressions, adopt observability principles from AgentOps work: it argues for tracing key agent artifacts and lifecycle data to detect anomalies and improve reliability. citeturn1search3turn1search18 For a steward, that means logging (at minimum):

- what was pruned/masked and why (rule, threshold, model score)
- what was summarized and the provenance pointer to raw history
- what was injected into a prompt (token budget accounting)
- downstream outcomes (turn count, tokens, test pass/fail)

### Concrete acceptance tests for “micro-compaction correctness”

You can make your steward testable by designing explicit “needles”:

- Place crucial constraints in earlier turns (“do not change API schema”, “must keep backward compatibility”, “tests must remain deterministic”).
- Run tasks that naturally create lots of low-value tool output (large `ripgrep` hits, long stack traces, big `npm install` logs).
- Verify the final patch still respects early constraints, and measure whether the steward retained and re-injected those constraints when needed.

This aligns with the documented reality that summaries are lossy and that you must preserve domain-critical information via targeted prompts or modular task structure. citeturn24view0turn25view0

## Practical synthesis: what to build first, and what to be careful about

The research and existing system hooks suggest a pragmatic “agentic compacting” roadmap that is low-risk and high-leverage.

First, implement **prune/mask plus structured working set** as the default. You have strong support for this approach:
- OpenCode already ships pruning as a first-class compaction control and has explicit pruning thresholds in its logic. citeturn4view4turn20view0  
- Research suggests masking can match summarization performance at a fraction of complexity and cost, and hybrids can do even better. citeturn16view2turn16view1  

Second, add **event-driven wakeups and bounded injection**. OpenCode’s plugin system exposes events and TUI hooks that allow you to determine precisely when to update state and when to append context near the user’s prompt. citeturn23view1turn23view0

Third, add **targeted micro-summaries with provenance**, but keep them reversible. The most important guardrail is to avoid turning your steward into a constant summarizer, because summaries can hide stop signals and increase agent turns. citeturn7view6turn1search1

Finally, integrate **repo-scoped shared memory** so multiple CLIs benefit from each other’s work without cross-project leakage. Claude Code’s memory architecture is explicit about per-repo auto memory directories and bounded session-start loading, giving you a natural place to store steward outputs that are stable and intentionally small. citeturn11view2turn11view0

The critical pushback, based on the evidence: do not treat “more summarization” as equivalent to “better compacting.” Summarization is valuable, but it is also a source of information loss and can lead to longer agent trajectories. citeturn24view0turn7view6 The steadiest path to “consistent conversations with less compaction” is not a hyperactive summarizer, but a steward that (a) aggressively prunes low-value tool noise, (b) externalizes durable state into small canonical blocks, and (c) uses summarization narrowly and with provenance.