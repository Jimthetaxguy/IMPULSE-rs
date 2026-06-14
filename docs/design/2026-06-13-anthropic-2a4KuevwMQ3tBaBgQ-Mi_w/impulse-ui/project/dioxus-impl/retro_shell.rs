//! retro_shell.rs — Dioxus 0.6 components for the Impulse Retro Broadcast UI.
//!
//! Bound to the real backend DTOs from `impulse-ops` and `impulse-desktop`:
//!   - `ProjectOpsSnapshot`  (project, agents, context, memory, retrieval)
//!   - `ContextHealthSummary` (tokens, usage_fraction, pending_review_count)
//!   - `AgentRuntime` / `AgentStatus`
//!
//! Nothing here is hardcoded: every number/label reads from the snapshot the
//! Rust backend feeds in through a `ReadOnlySignal<ProjectOpsSnapshot>`.
//!
//! Drop into impulse-desktop:  `mod theme; mod retro_shell;`
//! Render `RetroShell { snapshot }` in place of (or wrapping) `DesktopShell`.
//! See INTEGRATION.md for event wiring + asset registration.

use dioxus::prelude::*;
use impulse_ops::{AgentRuntime, ProjectOpsSnapshot};

use crate::theme::{format_count, status_dot_class, status_label};

/// Stylesheet shipped as a Dioxus asset. Falls back to inline if you prefer
/// `document::Style { {include_str!("../assets/impulse_crt.css")} }`.
const CRT_CSS: Asset = asset!("/assets/impulse_crt.css");

/// Top-level retro shell. `snapshot` is driven by the backend ops stream.
#[component]
pub fn RetroShell(snapshot: ReadOnlySignal<ProjectOpsSnapshot>) -> Element {
    let snap = snapshot.read();
    let ctx = &snap.context;

    let tokens = format_count(ctx.estimated_tokens);
    let window = format_count(ctx.window_tokens);
    let usage_pct = (ctx.usage_fraction * 100.0).round() as i32;

    let agents_online = snap.agents.iter().filter(|a| a.active).count();
    let working = snap
        .agents
        .iter()
        .filter(|a| matches!(a.agent_status, impulse_ops::AgentStatus::Working { .. }))
        .count();

    let retrieval_backend = snap.retrieval.backend.clone();
    let genome_decisions = snap.memory.genome_decisions;
    let pending = ctx.pending_review_count;
    let daemon_online = !snap.agents.is_empty();

    rsx! {
        document::Link { rel: "stylesheet", href: CRT_CSS }
        // Brand font (Baloo 2) + JetBrains Mono. Bundle locally for packaged builds.
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Baloo+2:wght@500;700;800&family=JetBrains+Mono:wght@400;500;700&display=swap",
        }

        main { class: "impulse-shell",
            header { class: "top-bar",
                div { class: "brand",
                    h1 { "impulse" }
                    span {
                        class: "daemon-state",
                        "data-state": if daemon_online { "online" } else { "offline" },
                        if daemon_online { "online · watching" } else { "daemon offline" }
                    }
                }
                nav { class: "command-surface",
                    button { class: "icon-button", title: "Command palette", "⌘K" }
                    button { class: "icon-button", title: "Review context", "Review" }
                    button { class: "icon-button", title: "Settings", "Settings" }
                }
            }

            div { class: "workspace-grid",
                // Left rail — sessions + live agent pool from the snapshot
                aside { class: "left-rail", "data-owner": "dioxus",
                    h2 { "Views" }
                    button { class: "rail-item active", "Terminal" }
                    button { class: "rail-item", "Memory" }
                    button { class: "rail-item", "Artifacts" }
                    button { class: "rail-item", "Supervisor" }

                    section { class: "agent-pool", "data-source": "agent_snapshot",
                        h2 { "Agents · {agents_online} online" }
                        for agent in snap.agents.iter() {
                            AgentRailItem { key: "{agent.id}", agent: agent.clone() }
                        }
                    }
                }

                // Center — brand hero + stats over the terminal stage
                section { class: "terminal-stage", "data-terminal-renderer": "xterm.js",
                    BrandHero {}
                    div { class: "stat-row",
                        Stat { k: "Memory",    v: tokens.clone(),       s: "tokens · {usage_pct}% of {window}" }
                        Stat { k: "Agents",    v: agents_online.to_string(), s: "online · {working} working" }
                        Stat { k: "Retrieval", v: retrieval_backend.clone(), s: "{genome_decisions} genome decisions" }
                    }
                    if pending > 0 {
                        PendingReview { count: pending }
                    }
                    div { class: "terminal-tabs", "data-owner": "dioxus",
                        for agent in snap.agents.iter().take(4) {
                            button {
                                class: if agent.active { "terminal-tab active" } else { "terminal-tab" },
                                "{agent.label}"
                            }
                        }
                    }
                    div {
                        id: "terminal-pane-primary",
                        class: "xterm-mount",
                        "data-xterm-mount": "true",
                        "data-agent-id": "shell",
                        "data-pty-owner": "rust-backend",
                        "data-command-bus": "agent_write",
                    }
                }

                // Right inspector — context health from the snapshot
                aside { class: "right-inspector", "data-owner": "dioxus",
                    section { class: "inspector-section",
                        h2 { "Context · {ctx.tier}" }
                        p { "{tokens} / {window} tokens · {ctx.injection_count} injections · {ctx.compaction_count} compactions" }
                    }
                    section { class: "inspector-section",
                        h2 { "Pending review" }
                        p {
                            if pending > 0 { "{pending} bundle(s) awaiting review-first apply" }
                            else { "Nothing pending. Memory is quiet." }
                        }
                    }
                    section { class: "inspector-section",
                        h2 { "Retrieval" }
                        p { "{snap.retrieval.mode} · {snap.retrieval.backend}" }
                    }
                }
            }

            footer { class: "event-strip", "data-owner": "dioxus",
                span { "ops_update {snap.generated_at}" }
                span { "{agents_online} agents" }
                span { "{snap.artifacts.len()} artifacts" }
                span { "{snap.interventions.len()} interventions" }
            }
        }
    }
}

#[component]
fn AgentRailItem(agent: AgentRuntime) -> Element {
    let dot = status_dot_class(&agent.agent_status);
    let label = status_label(&agent.agent_status);
    rsx! {
        button {
            class: if agent.active { "rail-item active" } else { "rail-item" },
            span { class: "dot {dot}" }
            "{agent.label}"
            span { style: "float:right;color:var(--c-label);font-size:10px;", "{label}" }
        }
    }
}

#[component]
fn Stat(k: String, v: String, s: String) -> Element {
    rsx! {
        div { class: "stat",
            div { class: "k", "{k}" }
            div { class: "v", "{v}" }
            div { class: "s", "{s}" }
        }
    }
}

#[component]
fn PendingReview(count: usize) -> Element {
    rsx! {
        div { class: "pending-bar",
            span { class: "label",
                span { class: "mark", "⏵" }
                "{count} injection(s) awaiting review"
            }
            span { class: "keys",
                b { "[a]" } " apply  " b { "[d]" } " diff  " b { "[s]" } " skip"
            }
        }
    }
}

/// Brand lockup: aperture-iris emblem + rocket + phosphor wordmark.
#[component]
fn BrandHero() -> Element {
    // 8 iris blades, hot phosphor hues cycling around the ring.
    let blade_colors = [
        "#ff8a1e", "#ff6a00", "#ffb01a", "#2fd6a8", "#2e7bff", "#5b63ff", "#2fd0ff", "#ff8a1e",
    ];
    let blades: Vec<(f64, f64, f64, &str)> = blade_colors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let a = (i as f64 / 8.0) * std::f64::consts::TAU;
            let (cx, cy, r) = (130.0, 130.0, 78.0);
            (cx + a.cos() * r, cy + a.sin() * r, a.to_degrees() + 90.0, *c)
        })
        .collect();

    rsx! {
        div { class: "crt-hero",
            // Emblem
            div { style: "position:relative;width:200px;height:200px;",
                svg {
                    width: "200", height: "200", view_box: "0 0 260 260",
                    style: "position:absolute;inset:0;",
                    for (i, (x, y, rot, color)) in blades.iter().enumerate() {
                        g {
                            key: "{i}",
                            class: "glow-soft",
                            transform: "translate({x},{y}) rotate({rot})",
                            rect {
                                x: "-9", y: "-30", width: "18", height: "52", rx: "3",
                                fill: "{color}",
                            }
                        }
                    }
                    circle {
                        cx: "130", cy: "130", r: "46",
                        fill: "none", stroke: "#ffb01a", stroke_width: "3",
                        style: "filter:drop-shadow(0 0 6px #ff6a00);",
                    }
                }
                // Rocket through the iris
                div { style: "position:absolute;inset:0;display:grid;place-items:center;",
                    svg {
                        width: "64", height: "99", view_box: "0 0 60 93", class: "glow-blue",
                        path { d: "M30 2 C40 14 44 30 44 48 L44 64 L16 64 L16 48 C16 30 20 14 30 2 Z", fill: "#5b63ff" }
                        circle { cx: "30", cy: "34", r: "8", fill: "#000" }
                        circle { cx: "30", cy: "34", r: "5", fill: "#2fd0ff" }
                        path { d: "M16 50 L4 70 L16 64 Z", fill: "#ff6a00" }
                        path { d: "M44 50 L56 70 L44 64 Z", fill: "#ff6a00" }
                        rect { x: "16", y: "64", width: "28", height: "6", fill: "#5b63ff" }
                        path { d: "M20 70 L30 92 L40 70 Z", fill: "#ffb01a" }
                        path { d: "M24 70 L30 84 L36 70 Z", fill: "#ff3b1f" }
                    }
                }
            }
            // Wordmark
            div { style: "text-align:left;",
                div { class: "brand-wordmark", "impulse" }
                div { class: "brand-tagline", "your ai remembers" }
            }
        }
    }
}
