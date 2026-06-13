//! Center-stage views for the Dioxus desktop shell.
//!
//! The shell renders one of these in the `terminal-stage` column based on the
//! active left-rail selection. The Terminal view (brand hero + xterm mounts)
//! lives in `ui.rs` and is kept alive across switches so the terminal interop
//! is never torn down; every other view lives here and binds directly to the
//! `ProjectOpsSnapshot` DTOs.
//!
//! Design contract: `docs/design/2026-06-13-anthropic-2a4KuevwMQ3tBaBgQ-Mi_w`.
//! - One focal point per view (P01): each view opens with a single hero band.
//! - Quiet by default, loud on signal (P04): data renders in calm cyan; only
//!   the Review "apply" affordance and critical interventions earn warm accent.
//! - Bloom is reserved for the brand lockup, never data screens (P05).
//!
//! Invariant: the non-Terminal views (Memory/Review/Artifacts/Supervisor) are
//! mounted only while selected (see the `match active` in `ui.rs`), so each
//! hardcodes the `active` CSS class — correct as long as that mount-when-active
//! contract holds. Only the Terminal view is kept alive across switches.

use dioxus::prelude::*;
use impulse_ops::{
    AgentRuntime, ArtifactEnvelope, ArtifactStatus, ArtifactViewHint, ContextHealthSummary,
    DelegationSummary, InsightRecord, InterventionRecommendation, MemorySummary, RetrievalSummary,
};

use crate::theme::{
    artifact_status_class, artifact_status_label, format_count, severity_class, status_dot_class,
    status_label, usage_meter_pct,
};

/// The five center-stage destinations reachable from the left-rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopView {
    Terminal,
    Memory,
    Review,
    Artifacts,
    Supervisor,
}

impl DesktopView {
    /// Rail order; also drives `ViewRail` iteration in `ui.rs`.
    pub const ALL: [DesktopView; 5] = [
        DesktopView::Terminal,
        DesktopView::Memory,
        DesktopView::Review,
        DesktopView::Artifacts,
        DesktopView::Supervisor,
    ];

    /// Human label shown on the rail button.
    pub fn label(self) -> &'static str {
        match self {
            DesktopView::Terminal => "Terminal",
            DesktopView::Memory => "Memory",
            DesktopView::Review => "Review",
            DesktopView::Artifacts => "Artifacts",
            DesktopView::Supervisor => "Supervisor",
        }
    }

    /// Stable slug used for `data-view` attributes and CSS hooks.
    pub fn slug(self) -> &'static str {
        match self {
            DesktopView::Terminal => "terminal",
            DesktopView::Memory => "memory",
            DesktopView::Review => "review",
            DesktopView::Artifacts => "artifacts",
            DesktopView::Supervisor => "supervisor",
        }
    }
}

/// A user intent emitted by an action button.
///
/// The shell records the latest intent in a status line. Wiring these to the
/// real `supervisor_local_action` Tauri command is a deliberate follow-up —
/// the command surface stays untouched in this redesign pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellIntent {
    ApplyInjection,
    SkipInjection,
    OpenInjectionDiff,
    ArtifactAction { artifact_id: String, action: String },
    ResolveIntervention { id: String },
}

impl ShellIntent {
    /// One-line description shown in the shell's status line.
    pub fn describe(&self) -> String {
        match self {
            ShellIntent::ApplyInjection => "apply injection bundle".to_string(),
            ShellIntent::SkipInjection => "skip injection bundle".to_string(),
            ShellIntent::OpenInjectionDiff => "open injection diff".to_string(),
            ShellIntent::ArtifactAction {
                artifact_id,
                action,
            } => format!("{action} on artifact {artifact_id}"),
            ShellIntent::ResolveIntervention { id } => {
                format!("resolve intervention {id}")
            }
        }
    }
}

fn view_hint_label(hint: &ArtifactViewHint) -> &'static str {
    match hint {
        ArtifactViewHint::SummaryCard => "summary",
        ArtifactViewHint::Table => "table",
        ArtifactViewHint::Timeline => "timeline",
        ArtifactViewHint::Diff => "diff",
        ArtifactViewHint::Log => "log",
        ArtifactViewHint::Markdown => "markdown",
        ArtifactViewHint::RawJson => "json",
    }
}

/// Ordering used when grouping artifacts: the things needing attention first.
const ARTIFACT_STATUS_ORDER: [ArtifactStatus; 5] = [
    ArtifactStatus::Pending,
    ArtifactStatus::Staged,
    ArtifactStatus::Ready,
    ArtifactStatus::Applied,
    ArtifactStatus::Acknowledged,
];

// ──────────────────────────── Shared primitives ────────────────────────────

#[component]
fn MeterBar(pct: i32, tone: String) -> Element {
    rsx! {
        div { class: "meter-bar",
            div { class: "meter-fill tone-{tone}", style: "width:{pct}%;" }
        }
    }
}

#[component]
fn StatCard(label: String, value: String, sub: String) -> Element {
    rsx! {
        div { class: "view-card",
            div { class: "view-card-k", "{label}" }
            div { class: "view-card-v", "{value}" }
            div { class: "view-card-s", "{sub}" }
        }
    }
}

#[component]
fn InsightStream(insights: Vec<InsightRecord>, empty_note: String) -> Element {
    rsx! {
        section { class: "view-section insight-stream", "data-source": "recent_insights",
            h3 { "Recent insights" }
            if insights.is_empty() {
                p { class: "section-empty", "{empty_note}" }
            } else {
                div { class: "insight-list",
                    for (i, insight) in insights.iter().enumerate() {
                        {
                            let when = insight
                                .timestamp
                                .clone()
                                .unwrap_or_else(|| "—".to_string());
                            let kind = if insight.kind.is_empty() {
                                "note"
                            } else {
                                insight.kind.as_str()
                            };
                            rsx! {
                                div { key: "{i}", class: "insight-row",
                                    span { class: "insight-when", "{when}" }
                                    span { class: "insight-kind", "{kind}" }
                                    span { class: "insight-agent", "{insight.agent_label}" }
                                    span { class: "insight-content", "{insight.content}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ──────────────────────────── Memory view ────────────────────────────

/// Context-health hero + genome/session cards + retrieval status + insight
/// stream. Calm cyan throughout — this is the everyday "what does my AI
/// remember" surface, never a bloom screen.
#[component]
pub fn MemoryView(
    context: ContextHealthSummary,
    memory: MemorySummary,
    retrieval: RetrievalSummary,
) -> Element {
    let pct = usage_meter_pct(context.usage_fraction);
    let tier = if context.tier.is_empty() {
        "idle"
    } else {
        context.tier.as_str()
    };
    let tokens = format_count(context.estimated_tokens);
    let window = format_count(context.window_tokens);
    let last_genome = memory
        .last_genome_update
        .clone()
        .unwrap_or_else(|| "never".to_string());
    let vector = if retrieval.vector_enabled {
        "on"
    } else {
        "off"
    };
    let strategy = if retrieval.semantic_strategy.is_empty() {
        "—"
    } else {
        retrieval.semantic_strategy.as_str()
    };
    let mode = if retrieval.mode.is_empty() {
        "—"
    } else {
        retrieval.mode.as_str()
    };
    let backend = if retrieval.backend.is_empty() {
        "—"
    } else {
        retrieval.backend.as_str()
    };

    rsx! {
        div { class: "stage-view view-memory active", "data-view": "memory",
            header { class: "view-hero",
                div { class: "view-eyebrow", "Context health" }
                div { class: "view-hero-row",
                    div { class: "view-hero-value", "{pct}%" }
                    div { class: "view-hero-meta",
                        div { class: "view-hero-tier", "tier · {tier}" }
                        div { class: "view-hero-sub", "{tokens} of {window} tokens" }
                    }
                }
                MeterBar { pct, tone: "cyan".to_string() }
            }
            div { class: "card-grid",
                StatCard {
                    label: "Genome".to_string(),
                    value: memory.genome_decisions.to_string(),
                    sub: format!("decisions · last {last_genome}"),
                }
                StatCard {
                    label: "Sessions".to_string(),
                    value: memory.active_sessions.to_string(),
                    sub: "active".to_string(),
                }
                StatCard {
                    label: "History".to_string(),
                    value: memory.history_entries.to_string(),
                    sub: "entries".to_string(),
                }
                StatCard {
                    label: "Injections".to_string(),
                    value: context.injection_count.to_string(),
                    sub: format!("{} compactions", context.compaction_count),
                }
            }
            section { class: "view-section retrieval-status", "data-source": "retrieval",
                h3 { "Retrieval" }
                div { class: "kv-grid",
                    div { class: "kv", span { class: "kv-k", "backend" } span { class: "kv-v", "{backend}" } }
                    div { class: "kv", span { class: "kv-k", "mode" } span { class: "kv-v", "{mode}" } }
                    div { class: "kv", span { class: "kv-k", "vector" } span { class: "kv-v", "{vector}" } }
                    div { class: "kv", span { class: "kv-k", "strategy" } span { class: "kv-v", "{strategy}" } }
                }
            }
            InsightStream {
                insights: context.recent_insights.clone(),
                empty_note: "No insights captured yet this session.".to_string(),
            }
        }
    }
}

// ──────────────────────────── Review view ────────────────────────────

/// The product's signature flow: review-before-apply. The injection bundle is
/// the hero — what *would* be added to context, shown as additive diff lines,
/// with an explicit apply / diff / skip action row. This is the only data
/// view allowed a warm accent (the apply affordance).
#[component]
pub fn ReviewView(
    pending: usize,
    insights: Vec<InsightRecord>,
    staged: Vec<ArtifactEnvelope>,
    on_intent: EventHandler<ShellIntent>,
) -> Element {
    let bundle_items = insights.len() + staged.len();
    let nothing = pending == 0 && bundle_items == 0;

    if nothing {
        return rsx! {
            div { class: "stage-view view-review active", "data-view": "review",
                div { class: "review-empty",
                    div { class: "review-empty-mark", "✓" }
                    div { class: "review-empty-title", "Memory is quiet" }
                    div { class: "review-empty-sub", "Nothing staged for review. Impulse is watching silently." }
                }
            }
        };
    }

    rsx! {
        div { class: "stage-view view-review active", "data-view": "review",
            header { class: "view-hero review-hero",
                div { class: "view-eyebrow", "Review before apply" }
                div { class: "view-hero-row",
                    div { class: "view-hero-value", "{pending}" }
                    div { class: "view-hero-meta",
                        div { class: "view-hero-tier", "bundle(s) awaiting review" }
                        div { class: "view-hero-sub",
                            "{insights.len()} insights · {staged.len()} staged artifacts"
                        }
                    }
                }
            }
            section { class: "view-section diff-preview", "data-source": "injection_bundle",
                h3 { "Would inject" }
                if bundle_items == 0 {
                    p { class: "section-empty", "Nothing in this bundle." }
                } else {
                    div { class: "diff-lines",
                        for (i, insight) in insights.iter().enumerate() {
                            {
                                let kind = if insight.kind.is_empty() {
                                    "note"
                                } else {
                                    insight.kind.as_str()
                                };
                                rsx! {
                                    div { key: "ins-{i}", class: "diff-line add",
                                        span { class: "diff-sign", "+" }
                                        span { class: "diff-kind", "{kind}" }
                                        span { class: "diff-agent", "{insight.agent_label}" }
                                        span { class: "diff-text", "{insight.content}" }
                                    }
                                }
                            }
                        }
                        for (i, artifact) in staged.iter().enumerate() {
                            {
                                let title = if artifact.title.is_empty() {
                                    artifact.kind.clone()
                                } else {
                                    artifact.title.clone()
                                };
                                rsx! {
                                    div { key: "art-{i}", class: "diff-line add staged",
                                        span { class: "diff-sign", "+" }
                                        span { class: "diff-kind", "{artifact_status_label(&artifact.status)}" }
                                        span { class: "diff-agent", "{artifact.agent_id}" }
                                        span { class: "diff-text", "{title}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "review-bundle-actions",
                button {
                    class: "action-apply",
                    onclick: move |_| on_intent.call(ShellIntent::ApplyInjection),
                    "apply"
                }
                button {
                    class: "action-ghost",
                    onclick: move |_| on_intent.call(ShellIntent::OpenInjectionDiff),
                    "diff"
                }
                button {
                    class: "action-ghost",
                    onclick: move |_| on_intent.call(ShellIntent::SkipInjection),
                    "skip"
                }
            }
        }
    }
}

// ──────────────────────────── Artifacts view ────────────────────────────

#[component]
fn ArtifactCard(artifact: ArtifactEnvelope, on_intent: EventHandler<ShellIntent>) -> Element {
    let title = if artifact.title.is_empty() {
        artifact.kind.clone()
    } else {
        artifact.title.clone()
    };
    rsx! {
        article { class: "artifact-card", "data-artifact-id": "{artifact.id}",
            header { class: "artifact-head",
                h4 { "{title}" }
                span { class: "artifact-badge {artifact_status_class(&artifact.status)}",
                    "{artifact_status_label(&artifact.status)}" }
            }
            if !artifact.summary.is_empty() {
                p { class: "artifact-summary", "{artifact.summary}" }
            }
            div { class: "artifact-meta",
                span { class: "artifact-kind", "{artifact.kind}" }
                span { class: "artifact-agent", "{artifact.agent_id}" }
            }
            if !artifact.view_hints.is_empty() {
                ul { class: "hint-chips",
                    for (i, hint) in artifact.view_hints.iter().enumerate() {
                        li { key: "{i}", "{view_hint_label(hint)}" }
                    }
                }
            }
            if !artifact.actions.is_empty() {
                div { class: "artifact-actions",
                    for action in artifact.actions.iter() {
                        {
                            let artifact_id = artifact.id.clone();
                            let action_id = action.id.clone();
                            let action_label = action.label.clone();
                            let class_name = if action.requires_confirmation {
                                "action-ghost mutating"
                            } else {
                                "action-ghost"
                            };
                            rsx! {
                                button {
                                    key: "{action_id}",
                                    class: "{class_name}",
                                    onclick: move |_| on_intent.call(ShellIntent::ArtifactAction {
                                        artifact_id: artifact_id.clone(),
                                        action: action_id.clone(),
                                    }),
                                    "{action_label}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Artifact envelopes grouped by lifecycle status, attention-first.
#[component]
pub fn ArtifactsView(
    artifacts: Vec<ArtifactEnvelope>,
    on_intent: EventHandler<ShellIntent>,
) -> Element {
    if artifacts.is_empty() {
        return rsx! {
            div { class: "stage-view view-artifacts active", "data-view": "artifacts",
                header { class: "view-hero",
                    div { class: "view-eyebrow", "Artifacts" }
                    div { class: "view-hero-row",
                        div { class: "view-hero-value", "0" }
                        div { class: "view-hero-meta",
                            div { class: "view-hero-tier", "envelopes" }
                            div { class: "view-hero-sub", "No artifacts produced yet." }
                        }
                    }
                }
            }
        };
    }

    rsx! {
        div { class: "stage-view view-artifacts active", "data-view": "artifacts",
            header { class: "view-hero",
                div { class: "view-eyebrow", "Artifacts" }
                div { class: "view-hero-row",
                    div { class: "view-hero-value", "{artifacts.len()}" }
                    div { class: "view-hero-meta",
                        div { class: "view-hero-tier", "envelopes" }
                        div { class: "view-hero-sub", "grouped by lifecycle status" }
                    }
                }
            }
            for status in ARTIFACT_STATUS_ORDER {
                {
                    let group: Vec<ArtifactEnvelope> = artifacts
                        .iter()
                        .filter(|a| a.status == status)
                        .cloned()
                        .collect();
                    if group.is_empty() {
                        rsx! {}
                    } else {
                        let group_label = artifact_status_label(&status);
                        rsx! {
                            section { class: "view-section artifact-group", "data-status": "{group_label}",
                                h3 { "{group_label}" span { class: "group-count", "{group.len()}" } }
                                div { class: "artifact-grid",
                                    for artifact in group.iter() {
                                        ArtifactCard {
                                            key: "{artifact.id}",
                                            artifact: artifact.clone(),
                                            on_intent,
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ──────────────────────────── Supervisor view ────────────────────────────

#[component]
fn InterventionCard(
    rec: InterventionRecommendation,
    on_intent: EventHandler<ShellIntent>,
) -> Element {
    let id = rec.id.clone();
    let target = rec.target_agent_id.clone().unwrap_or_default();
    let action_label = if rec.action_label.is_empty() {
        "resolve".to_string()
    } else {
        rec.action_label.clone()
    };
    rsx! {
        article { class: "intervention-card {severity_class(&rec.severity)}",
            "data-intervention-id": "{rec.id}",
            header { class: "intervention-head",
                span { class: "sev-badge", "{rec.severity}" }
                h4 { "{rec.title}" }
            }
            if !rec.description.is_empty() {
                p { class: "intervention-desc", "{rec.description}" }
            }
            div { class: "intervention-foot",
                if !target.is_empty() {
                    span { class: "intervention-target", "→ {target}" }
                }
                button {
                    class: "action-ghost",
                    onclick: move |_| on_intent.call(ShellIntent::ResolveIntervention { id: id.clone() }),
                    "{action_label}"
                }
            }
        }
    }
}

#[component]
fn DelegationRow(delegation: DelegationSummary) -> Element {
    let diff_text = match delegation.diff_summary.as_ref() {
        Some(d) => format!(
            "+{} −{} · {} files",
            d.lines_added, d.lines_removed, d.files_changed
        ),
        None => "no diff".to_string(),
    };
    let worker = delegation
        .worker_pane_id
        .map(|w| w.to_string())
        .unwrap_or_else(|| "—".to_string());
    rsx! {
        div { class: "delegation-row", "data-delegation-id": "{delegation.id}",
            span { class: "delegation-task", "{delegation.task}" }
            span { class: "delegation-state", "{delegation.state}" }
            span { class: "delegation-route", "{delegation.coordinator_pane_id} → {worker}" }
            span { class: "delegation-diff", "{diff_text}" }
        }
    }
}

/// Operator board: interventions (severity-coded), delegations, and an agent
/// status table. This is the densest view — Operator density mode.
#[component]
pub fn SupervisorView(
    interventions: Vec<InterventionRecommendation>,
    delegations: Vec<DelegationSummary>,
    agents: Vec<AgentRuntime>,
    on_intent: EventHandler<ShellIntent>,
) -> Element {
    let headline = if interventions.is_empty() {
        "clear".to_string()
    } else {
        interventions.len().to_string()
    };
    rsx! {
        div { class: "stage-view view-supervisor active", "data-view": "supervisor",
            header { class: "view-hero",
                div { class: "view-eyebrow", "Operator board" }
                div { class: "view-hero-row",
                    div { class: "view-hero-value", "{headline}" }
                    div { class: "view-hero-meta",
                        div { class: "view-hero-tier", "interventions" }
                        div { class: "view-hero-sub",
                            "{agents.len()} agents · {delegations.len()} delegations"
                        }
                    }
                }
            }
            section { class: "view-section interventions", "data-source": "interventions",
                h3 { "Interventions" }
                if interventions.is_empty() {
                    p { class: "section-empty", "No interventions recommended." }
                } else {
                    div { class: "intervention-grid",
                        for rec in interventions.iter() {
                            InterventionCard { key: "{rec.id}", rec: rec.clone(), on_intent }
                        }
                    }
                }
            }
            section { class: "view-section delegations", "data-source": "delegations",
                h3 { "Delegations" }
                if delegations.is_empty() {
                    p { class: "section-empty", "No active delegations." }
                } else {
                    div { class: "delegation-list",
                        for delegation in delegations.iter() {
                            DelegationRow { key: "{delegation.id}", delegation: delegation.clone() }
                        }
                    }
                }
            }
            section { class: "view-section agent-board", "data-source": "agents",
                h3 { "Agents" }
                if agents.is_empty() {
                    p { class: "section-empty", "No agents running." }
                } else {
                    div { class: "agent-board-list",
                        for agent in agents.iter() {
                            {
                                let task = agent.current_task.clone().unwrap_or_default();
                                let diff = match agent.diff_summary.as_ref() {
                                    Some(d) => format!("+{} −{}", d.lines_added, d.lines_removed),
                                    None => String::new(),
                                };
                                rsx! {
                                    div { key: "{agent.id}", class: "agent-board-row",
                                        "data-agent-id": "{agent.id}",
                                        span { class: "dot {status_dot_class(&agent.agent_status)}" }
                                        span { class: "agent-board-label", "{agent.label}" }
                                        span { class: "agent-board-status", "{status_label(&agent.agent_status)}" }
                                        span { class: "agent-board-task", "{task}" }
                                        span { class: "agent-board-diff", "{diff}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_view_all_has_five_in_rail_order() {
        assert_eq!(
            DesktopView::ALL,
            [
                DesktopView::Terminal,
                DesktopView::Memory,
                DesktopView::Review,
                DesktopView::Artifacts,
                DesktopView::Supervisor,
            ]
        );
        for view in DesktopView::ALL {
            assert!(!view.label().is_empty());
            assert!(!view.slug().is_empty());
        }
        assert_eq!(DesktopView::Terminal.slug(), "terminal");
        assert_eq!(DesktopView::Memory.label(), "Memory");
        assert_eq!(DesktopView::Artifacts.slug(), "artifacts");
        assert_eq!(DesktopView::Supervisor.label(), "Supervisor");
    }

    #[test]
    fn test_shell_intent_describe_is_human_readable() {
        assert_eq!(
            ShellIntent::ApplyInjection.describe(),
            "apply injection bundle"
        );
        assert_eq!(
            ShellIntent::ArtifactAction {
                artifact_id: "art-1".to_string(),
                action: "apply".to_string(),
            }
            .describe(),
            "apply on artifact art-1"
        );
        assert_eq!(
            ShellIntent::ResolveIntervention {
                id: "iv-9".to_string()
            }
            .describe(),
            "resolve intervention iv-9"
        );
        assert_eq!(
            ShellIntent::SkipInjection.describe(),
            "skip injection bundle"
        );
        assert_eq!(
            ShellIntent::OpenInjectionDiff.describe(),
            "open injection diff"
        );
    }

    #[test]
    fn test_view_hint_label_covers_every_variant() {
        assert_eq!(view_hint_label(&ArtifactViewHint::SummaryCard), "summary");
        assert_eq!(view_hint_label(&ArtifactViewHint::Table), "table");
        assert_eq!(view_hint_label(&ArtifactViewHint::Timeline), "timeline");
        assert_eq!(view_hint_label(&ArtifactViewHint::Diff), "diff");
        assert_eq!(view_hint_label(&ArtifactViewHint::Log), "log");
        assert_eq!(view_hint_label(&ArtifactViewHint::Markdown), "markdown");
        assert_eq!(view_hint_label(&ArtifactViewHint::RawJson), "json");
    }

    #[test]
    fn test_artifact_status_order_is_attention_first() {
        assert_eq!(
            ARTIFACT_STATUS_ORDER,
            [
                ArtifactStatus::Pending,
                ArtifactStatus::Staged,
                ArtifactStatus::Ready,
                ArtifactStatus::Applied,
                ArtifactStatus::Acknowledged,
            ]
        );
    }
}
