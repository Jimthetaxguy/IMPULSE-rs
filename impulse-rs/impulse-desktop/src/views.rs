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
//! Invariant: the non-Terminal views (Memory/Review/Artifacts/Oversight) are
//! mounted only while selected (see the `match active` in `ui.rs`), so each
//! hardcodes the `active` CSS class — correct as long as that mount-when-active
//! contract holds. Only the Terminal view is kept alive across switches.

use dioxus::prelude::*;
use impulse_ops::memory_candidate::{
    AcceptedRunMemoryCandidate, AcceptedRunSourceAssurance, MemoryCandidateStatus,
};
use impulse_ops::{
    ArtifactEnvelope, ArtifactStatus, ArtifactViewHint, ContextHealthSummary, InsightRecord,
    MemorySummary, RetrievalSummary,
};

use crate::theme::{artifact_status_class, artifact_status_label, format_count, usage_meter_pct};

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
            DesktopView::Supervisor => "Oversight",
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
    memory_candidates: Vec<AcceptedRunMemoryCandidate>,
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
                StatCard {
                    label: "Candidates".to_string(),
                    value: memory_candidates.len().to_string(),
                    sub: "review-only accepted outcomes".to_string(),
                }
            }
            section { class: "view-section memory-candidates", "data-source": "memory_candidates",
                h3 { "Accepted-run candidates" }
                if memory_candidates.is_empty() {
                    p { class: "section-empty", "No accepted outcome is awaiting semantic-memory review." }
                } else {
                    div { class: "memory-candidate-list",
                        for candidate in memory_candidates {
                            {
                                let assurance = match candidate.source_assurance {
                                    AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator =>
                                        "daemon-profiled evidence · declared operator",
                                    AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator =>
                                        "caller-composed evidence · declared operator",
                                };
                                let status = match candidate.status {
                                    MemoryCandidateStatus::PendingReview => "Pending review — not stored in GENOME",
                                };
                                rsx! {
                                    article {
                                        class: "memory-candidate-card",
                                        "data-candidate-id": "{candidate.id}",
                                        div { class: "view-card-k", "{status}" }
                                        h4 { "{candidate.task}" }
                                        p { class: "memory-candidate-summary", "{candidate.proposed_summary}" }
                                        dl { class: "memory-candidate-evidence",
                                            dt { "Verified subject" }
                                            dd { code { "{candidate.subject_revision}" } }
                                            dt { "Verification" }
                                            dd { "{candidate.verification_policy}" }
                                            dt { "Assurance" }
                                            dd { "{assurance}" }
                                            dt { "Task / evidence" }
                                            dd {
                                                code { "{candidate.governed_task_id}" }
                                                " · "
                                                code { "{candidate.verification_id}" }
                                                " · "
                                                code { "{candidate.operator_decision_id}" }
                                            }
                                        }
                                        ul { class: "memory-candidate-criteria",
                                            for criterion in candidate.acceptance_criteria {
                                                li { "{criterion}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
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

// ──────────────────────────── Artifacts view ────────────────────────────

#[component]
fn ArtifactCard(artifact: ArtifactEnvelope) -> Element {
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
        }
    }
}

/// Read-only artifact envelopes grouped by lifecycle status, attention-first.
///
/// Envelopes retain their declared actions in the shared DTO, but the desktop
/// does not render them until a real host-command contract can execute them.
#[component]
pub fn ArtifactsView(artifacts: Vec<ArtifactEnvelope>) -> Element {
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
        assert_eq!(DesktopView::Supervisor.label(), "Oversight");
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
