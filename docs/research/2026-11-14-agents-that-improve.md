> Source: "The architecture of agents that actually improve", given by Jim via Grok-Prime on Nov 14, 2026. The text below is verbatim; only the line-number prefixes in the uploaded copy were removed.

The architecture of agents that actually improve

Unifying Goal Engineering, SoMem, WikiTrace, and the Agent Harness

The research we’ve been examining points toward a larger architectural shift.

The next generation of agents should be designed around their ability to improve through experience, while preserving the evidence needed to determine whether that improvement is real.

This changes how we think about agent boundaries, memory, execution, feedback, and evaluation.

The four ideas fit together:

* Goal Engineering (Harrison Chase): Define what an agent is responsible for, how success is measured, and how humans can efficiently teach it.
* SoMem: Allow agents to organize accumulated knowledge into evolving, navigable memory rather than relying exclusively on fixed schemas or isolated retrieval chunks.
* WikiTrace: Keep learned procedures connected to their original execution evidence so agents can challenge remembered lessons before acting on them.
* Harness Engineering: Provide the execution, authorization, persistence, verification, and rollback mechanisms that make this behavior operationally reliable.

Each solves a different part of the problem. The architecture becomes much more interesting when they operate as one system.

1. The complete architecture

Evidence-grounded adaptive agent platform

Proposed integrated design. The underlying papers study individual mechanisms, not this complete system.

The essential loop is:

Define the goal → Execute work → Preserve evidence → Organize knowledge → Obtain expert feedback → Validate a revision → Apply it to future work → Measure whether performance improves.

The last step feeds back into the next execution. That is what turns memory from a storage feature into a mechanism for controlled improvement.

2. The five architectural decisions that matter

A. Separate ownership, execution, review, and learning

Harrison Chase’s goal engineering argument is strongest when treated as a way to organize feedback, rather than a requirement that every agent correspond to one business process.

Consider an enterprise sales platform.

An account is a natural unit of ownership. But a salesperson might want to review outbound emails across 20 accounts in one sitting because the same strategic judgment applies to each.

That gives us four different scopes:

Scope	Example	Responsibility
Outcome	Customer account	Achieve the overall business objective
Execution	Draft a follow-up	Complete a bounded action
Review	Outbound messaging motion	Collect consistent expert feedback
Learning	Sales communication method	Improve future work across accounts

These should be independently configurable.

The same logic applies to tax. An engagement owns the deliverable, specialist workers perform the analysis, reviewers evaluate comparable decisions, and approved methods can transfer across engagements.

The reusable unit should be the method, not necessarily the agent instance.

B. Treat memory as a hierarchy of authority

SoMem and WikiTrace both preserve access to original information, but they do so for different reasons.

SoMem uses source-linked topic documents to support long-term recall. WikiTrace links procedural insights to execution evidence so an agent can investigate whether a lesson is valid.

I would introduce an explicit distinction between historical evidence and current authority.

Memory class	Can change?	How it should be trusted
Raw events and observations	Append-only, with governed corrections and deletion	Evidence of what was recorded
Organized topics	Yes, with revisions	Useful interpretations that can be checked
Procedural lessons	Versioned	Conditional guidance requiring applicability checks
Current authoritative state	Through controlled transitions	Accepted state within a defined scope
Working context	Reconstructed for each run	Temporary selection of relevant information

An immutable execution trace is authoritative about what was recorded during that execution. It is not necessarily authoritative about the outside world.

Likewise, an approved method may govern how an agent performs a task, but it does not make every conclusion produced by that method correct.

This prevents the system from confusing what happened, what it inferred, and what it is authorized to do.

C. Make memory writes transactional

This is where your Rust architecture becomes particularly relevant.

I would use a single authoritative commit path for memory mutations, with multiple agents allowed to propose changes.

Proposed memory commit protocol

1. Observe an event or correction
2. Propose a memory revision
3. Attach source references and scope
4. Validate schema, links and permissions
5. Check the expected memory version
6. Run applicable semantic evaluations
7. Commit the approved revision
8. Rebuild derived search and graph indices
9. Publish a versioned memory snapshot

This is compatible with the single-writer, append-only ledger direction you’ve been exploring.

SQLite with WAL could be sufficient for a local or single-host implementation. Multiple independent hosts would need a coordinated commit authority or another mechanism that prevents conflicting writes.

The key is that the model can propose semantic changes while deterministic software controls whether those changes become durable.

A model should never be able to bypass access rules simply by writing a convincing Markdown file.

D. Treat retrieved memories as hypotheses

WikiTrace’s most important conceptual contribution is that retrieved memory should not automatically become instruction.

A lesson might be incorrect, outdated, inapplicable, or less specific than another lesson.

A robust Reader should distinguish:

* Relevant and adequately supported guidance that can be applied.
* Potentially relevant guidance that requires source inspection.
* Conflicting guidance that requires reconciliation.
* Guidance that does not apply and should be excluded.

The amount of verification should depend on the consequence of the decision.

For a low-risk formatting preference, retrieving the stored preference may be sufficient. For a privileged tool action, financial calculation, or substantive professional judgment, the agent may need to inspect original evidence and check current conditions.

This creates a practical form of risk-adaptive memory retrieval.

E. Measure improvement and damage separately

The most important WikiTrace evaluation idea is distinguishing newly successful tasks from previously successful tasks that memory causes to fail.

Suppose a baseline agent succeeds on 80 out of 100 tasks.

After adding memory, it succeeds on 86.

That sounds like a six-point improvement. But the underlying result might be:

Illustrative memory evaluation

Newly successful tasks

+10

Previously successful tasks lost

−4

Net improvement

+6

Hypothetical example, not a result from either paper.

The net improvement hides four regressions.

I would make regression rate a release criterion for any memory or method update. Evaluate against the previous system, not only a static no-memory baseline, and account for ordinary run-to-run variation.

This also creates a foundation for autonomous improvement: the system can propose changes, but promotion depends on demonstrated benefit and acceptable downside.

3. What this means for your Rust agent ecosystem

The most productive design is a shared set of primitives, not three separate implementations of memory.

I would divide the responsibilities as follows. These are proposed roles, not claims about the current repository implementations.

Component	Proposed responsibility
min-agent-rs	Minimal execution loop, tool calls, bounded context retrieval, source inspection
IMPULSE	Orchestration, goal contracts, task dependencies, execution budgets, feedback routing
ROSA	Persistent state, evidence-linked memory, revision proposals, controlled knowledge reuse
Shared Rust crates	Event ledger, provenance IDs, capability checks, versioned commits, evaluation interfaces

The shared layer matters most. Without it, each agent framework will eventually invent incompatible definitions of memory, state, evidence, and authority.

I would start with a few common Rust types:

struct MemoryReference {
    source_ids: Vec<SourceId>,
    scope: MemoryScope,
    revision: RevisionId,
}
struct LearnedMethod {
    id: MethodId,
    applicability: Applicability,
    evidence: Vec<MemoryReference>,
    status: MethodStatus,
}
struct GoalContract {
    objective: Objective,
    constraints: Vec<Constraint>,
    success: SuccessCriteria,
    budget: ExecutionBudget,
}

These are conceptual interfaces. The difficult work lies in defining their semantics, authorization rules, and state transitions.

Your earlier interest in Verus fits naturally at this boundary. Formal verification could help establish properties of the deterministic infrastructure, such as version consistency or authorization-preserving transitions, without pretending to prove the correctness of arbitrary LLM judgments.

4. The bigger opportunity: agents that accumulate organizational expertise

This is where I think the research becomes commercially significant.

Today, much of the value created by an agent disappears after the task finishes.

The agent may have discovered a useful procedure, encountered an unusual failure, learned a user’s preference, or received a correction from an expert. But unless the system preserves and applies that knowledge appropriately, the next agent repeats the same work.

The combined architecture provides a way to retain that value.

Consider a tax platform:

* A specialist agent prepares a workpaper.
    It gathers evidence, applies an existing method, and proposes conclusions.
* A practitioner corrects the analysis.
    The system captures what changed, why it changed, and which evidence supports the correction.
* A memory writer proposes a reusable lesson.
    It distinguishes a client-specific fact from a general preparation or review procedure.
* The system validates the proposed lesson.
    It checks provenance, applicability, authorization, and performance on representative cases.
* Future engagements can use the approved method.
    They reuse the procedure while independently evaluating their own facts and evidence.
* The platform measures the consequences.
    It tracks review time, supported completion, unnecessary escalations, errors, and regressions.

This is how a platform could progressively capture the practical expertise of its users without automatically treating every user correction as a universal rule.

It also explains why the feedback interface matters as much as the underlying memory engine. If expert corrections are expensive to provide or impossible to interpret, there is little high-quality knowledge to preserve.

5. The research questions I would prioritize

There are three questions that the combined papers do not fully answer.

First, can an agent reliably infer the scope of a correction? Distinguishing a one-time exception from a reusable procedure is essential to preventing harmful generalization.

Second, can memory revisions improve performance over long periods without accumulating contradictions? SoMem addresses organization and WikiTrace addresses evidence inspection, but neither establishes an indefinitely stable learning system.

Third, can the system optimize globally when learning occurs locally? A specialized motion may become better at its assigned task while damaging the larger engagement or business objective.

These are strong candidates for experiments because they test the proposed architecture rather than merely replicating benchmark scores.

6. Recommended implementation sequence

Phase 1: Evidence-linked memory

Build immutable event storage, versioned Markdown knowledge, stable source references, and deterministic validation. Prove that an agent can retrieve a lesson and inspect its origin.

Phase 2: Memory regression harness

Compare no memory, distilled memory, evidence-linked memory, and selective retrieval on the same tasks. Track gains, regressions, latency, and cost.

Phase 3: Feedback-driven method revisions

Add review queues, correction classification, applicability conditions, staged method updates, approval, and rollback.

Phase 4: Multi-agent knowledge transfer

Let multiple agents use approved methods while maintaining separate execution state, authorization, and outcome ownership. Evaluate whether shared learning improves end-to-end results.

I would resist building a sophisticated autonomous memory optimizer before Phase 2 works. The central risk is creating a system that becomes increasingly confident in accumulated mistakes.

The unifying thesis

The conventional agent architecture is organized around a model, tools, and a context window.

The architecture emerging from this research is organized around goals, evidence, experience, and controlled adaptation.

That changes the definition of a successful agent platform.

It is no longer sufficient for an agent to complete a task once. The system should be able to explain what it learned, preserve the evidence behind that learning, determine when the lesson applies, and establish whether using it makes future work better.

For your broader agent architecture, I would make one principle foundational:

Agents can propose knowledge. Evidence supports knowledge. Evaluation tests knowledge. Authority governs its adoption.

That principle connects Chase’s goal engineering, SoMem’s self-organizing memory, WikiTrace’s evidence-linked procedural learning, and your Rust-based harness design into one coherent system.

The long-term ambition is not simply agents with better memory. It is a platform where human expertise and agent experience can accumulate into reusable, inspectable, and increasingly reliable methods of work.
