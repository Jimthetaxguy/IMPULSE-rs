//! Component-source only for the Tauri desktop shell migration.
//! The mounted runtime remains `src-tauri/ui` during this phase.

use dioxus::prelude::*;

// Keep the prototype in step with the mounted shell's semantic rhythm.
const FONT_MONO: &str = "'SF Mono', 'IBM Plex Mono', monospace";
const SPACE_1: &str = "8px";
const SPACE_2: &str = "10px";
const SPACE_3: &str = "12px";
const SPACE_4: &str = "14px";
const SPACE_5: &str = "16px";
const SPACE_6: &str = "18px";
const SPACE_7: &str = "22px";
const TYPE_EYEBROW: &str = "11px";
const TYPE_CAPTION: &str = "12px";
const TYPE_BODY: &str = "13px";
const TYPE_TITLE: &str = "14px";
const TYPE_DISPLAY: &str = "clamp(20px, 1.1rem + 0.5vw, 24px)";
const LEADING_TIGHT: &str = "1.35";
const LEADING_COPY: &str = "1.55";
const LAYOUT_RAIL: &str = "clamp(240px, 22vw, 280px)";
const LAYOUT_INSPECTOR: &str = "clamp(288px, 25vw, 320px)";
const MEASURE_COPY: &str = "68ch";

#[component]
pub fn DesktopShellPrototype() -> Element {
    let root_style = format!(
        "min-height: 100vh; background: linear-gradient(180deg, #101522 0%, #090d16 100%); color: #ecf2ff; font-family: {FONT_MONO}; font-size: {TYPE_BODY}; line-height: {LEADING_COPY}; display: flex; flex-direction: column;"
    );
    let topbar_style = format!(
        "display: flex; align-items: center; justify-content: space-between; gap: {SPACE_5}; padding: {SPACE_6} {SPACE_7}; border-bottom: 1px solid rgba(143, 181, 255, 0.18); background: rgba(9, 13, 22, 0.74); backdrop-filter: blur(18px);"
    );
    let title_style = format!(
        "margin: 0; font-size: {TYPE_DISPLAY}; line-height: {LEADING_TIGHT}; letter-spacing: 0.04em; max-width: {MEASURE_COPY};"
    );
    let subtitle_style = format!(
        "margin: 4px 0 0; color: #91a4c8; font-size: {TYPE_CAPTION}; line-height: {LEADING_COPY}; max-width: {MEASURE_COPY};"
    );
    let topbar_pill_row_style = format!("display: flex; gap: {SPACE_2};");
    let workspace_style = format!(
        "display: grid; grid-template-columns: {LAYOUT_RAIL} minmax(0, 1fr) {LAYOUT_INSPECTOR}; gap: {SPACE_4}; padding: {SPACE_4}; flex: 1 1 auto;"
    );
    let footer_style = format!(
        "display: flex; gap: {SPACE_2}; padding: {SPACE_3} {SPACE_4} {SPACE_5}; border-top: 1px solid rgba(143, 181, 255, 0.12); background: rgba(7, 10, 17, 0.86);"
    );

    rsx! {
        div {
            style: root_style,
            div {
                style: topbar_style,
                div {
                    h1 { style: title_style, "Impulse Desktop Shell" }
                    p { style: subtitle_style, "STATUS: PARTIAL — Dioxus component source only; mounted runtime stays in src-tauri/ui." }
                }
                div {
                    style: topbar_pill_row_style,
                    StatusPill { label: "Workspace /Users/jamespustorino/Desktop/VibeCode_Prime/CLI_CU_L8R".to_string(), tone: "#2f7ddb".to_string() }
                    StatusPill { label: "Mounted runtime: src-tauri/ui".to_string(), tone: "#17894f".to_string() }
                }
            }
            div {
                style: workspace_style,
                Rail {}
                CenterShell {}
                Inspector {}
            }
            div {
                style: footer_style,
                StatusPill { label: "Actions: open terminal, focus pane, review artifact".to_string(), tone: "#6e5bd0".to_string() }
                StatusPill { label: "Next: land terminal renderer + daemon-backed panels".to_string(), tone: "#a86919".to_string() }
            }
        }
    }
}

#[component]
fn Rail() -> Element {
    let rail_style = format!(
        "display: flex; flex-direction: column; gap: {SPACE_3}; border: 1px solid rgba(143, 181, 255, 0.12); border-radius: 20px; padding: {SPACE_5}; background: rgba(15, 21, 34, 0.78);"
    );

    rsx! {
        div {
            style: rail_style,
            SectionHeading { title: "Sessions".to_string(), subtitle: "Projects / agents / recents".to_string() }
            RailCard { title: "Impulse".to_string(), meta: "3 live sessions · 1 blocked".to_string() }
            RailCard { title: "Harness".to_string(), meta: "Codex + Claude Code + OpenCode".to_string() }
            RailCard { title: "Artifacts".to_string(), meta: "4 pending review actions".to_string() }
            SectionHeading { title: "Shortcuts".to_string(), subtitle: "Keyboard-first shell".to_string() }
            ShortcutRow { keys: "⌘T".to_string(), label: "Open terminal".to_string() }
            ShortcutRow { keys: "⌘⇧]".to_string(), label: "Next pane".to_string() }
            ShortcutRow { keys: "⌘K".to_string(), label: "Command palette".to_string() }
        }
    }
}

#[component]
fn CenterShell() -> Element {
    let center_style = format!("display: flex; flex-direction: column; gap: {SPACE_3};");
    let tabs_style = format!("display: flex; gap: {SPACE_1};");
    let terminal_stack_style = format!(
        "display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(18rem, 0.9fr); gap: {SPACE_3}; flex: 1 1 auto;"
    );

    rsx! {
        div {
            style: center_style,
            div {
                style: tabs_style,
                Tab { label: "Claude Code · planning".to_string(), active: true }
                Tab { label: "Codex · docs reset".to_string(), active: false }
                Tab { label: "Shell · benchmarks".to_string(), active: false }
            }
            div {
                style: terminal_stack_style,
                TerminalPane {
                    title: "Primary Terminal".to_string(),
                    subtitle: "PTY-backed pane — bridge live, renderer still placeholder in the mounted shell".to_string(),
                    body: "$ impulse desktop shell\n$ status\nDaemon: connected\nDesktop mode: live PTY foundation\nNext step: terminal renderer + ops inspector".to_string(),
                }
                TerminalPane {
                    title: "Secondary Terminal".to_string(),
                    subtitle: "Parallel session / preview / tail in the component source".to_string(),
                    body: "$ panes\n1. planning\n2. docs reset\n3. benchmark prep\n\nInspector stays daemon-backed.".to_string(),
                }
            }
        }
    }
}

#[component]
fn Inspector() -> Element {
    let inspector_style = format!(
        "display: flex; flex-direction: column; gap: {SPACE_3}; border: 1px solid rgba(143, 181, 255, 0.12); border-radius: 20px; padding: {SPACE_5}; background: rgba(13, 18, 30, 0.82);"
    );

    rsx! {
        div {
            style: inspector_style,
            SectionHeading { title: "Inspector".to_string(), subtitle: "Context / artifacts / supervisor".to_string() }
            InfoBlock { title: "Context health".to_string(), detail: "Warm tier · 17.4k tokens · 2 suggestions".to_string() }
            InfoBlock { title: "Artifacts".to_string(), detail: "handoff-20260415.md · 2 apply actions".to_string() }
            InfoBlock { title: "Supervisor".to_string(), detail: "1 warning · 3 queued recommendations".to_string() }
            InfoBlock { title: "Bridge state".to_string(), detail: "Component source mirrors the shell layout; the mounted runtime remains src-tauri/ui until the frontend migration lands".to_string() }
        }
    }
}

#[component]
fn TerminalPane(title: String, subtitle: String, body: String) -> Element {
    let pane_style = "border: 1px solid rgba(143, 181, 255, 0.16); border-radius: 22px; background: linear-gradient(180deg, rgba(11, 16, 27, 0.96) 0%, rgba(7, 10, 17, 0.92) 100%); min-height: 420px; display: flex; flex-direction: column;".to_string();
    let pane_head_style = format!(
        "display: flex; align-items: center; justify-content: space-between; gap: {SPACE_3}; padding: {SPACE_4} {SPACE_5}; border-bottom: 1px solid rgba(143, 181, 255, 0.10);"
    );
    let pane_title_style = format!(
        "margin: 0; font-size: {TYPE_TITLE}; line-height: {LEADING_TIGHT}; letter-spacing: 0.04em;"
    );
    let pane_subtitle_style = format!(
        "margin: 4px 0 0; font-size: {TYPE_CAPTION}; line-height: {LEADING_COPY}; color: #88a0c2; max-width: {MEASURE_COPY};"
    );
    let pane_body_style = format!(
        "margin: 0; padding: {SPACE_6} {SPACE_5}; color: #dce8ff; flex: 1 1 auto; white-space: pre-wrap; line-height: {LEADING_COPY};"
    );

    rsx! {
        div {
            style: pane_style,
            div {
                style: pane_head_style,
                div {
                    h2 { style: pane_title_style, "{title}" }
                    p { style: pane_subtitle_style, "{subtitle}" }
                }
                StatusPill { label: "Focused".to_string(), tone: "#17894f".to_string() }
            }
            pre {
                style: pane_body_style,
                "{body}"
            }
        }
    }
}

#[component]
fn SectionHeading(title: String, subtitle: String) -> Element {
    let title_style = format!(
        "margin: 0; font-size: {TYPE_EYEBROW}; line-height: {LEADING_TIGHT}; text-transform: uppercase; letter-spacing: 0.12em; color: #8bb6ff;"
    );
    let subtitle_style = format!(
        "margin: 4px 0 0; font-size: {TYPE_CAPTION}; line-height: {LEADING_COPY}; color: #88a0c2;"
    );

    rsx! {
        div {
            h2 { style: title_style, "{title}" }
            p { style: subtitle_style, "{subtitle}" }
        }
    }
}

#[component]
fn RailCard(title: String, meta: String) -> Element {
    let card_style = format!(
        "padding: {SPACE_3}; border-radius: 16px; background: rgba(255, 255, 255, 0.03); border: 1px solid rgba(143, 181, 255, 0.08);"
    );
    let meta_style = format!(
        "margin: 6px 0 0; font-size: {TYPE_CAPTION}; line-height: {LEADING_COPY}; color: #91a4c8;"
    );

    rsx! {
        div {
            style: card_style,
            h3 { style: format!("margin: 0; font-size: {TYPE_TITLE}; line-height: {LEADING_TIGHT};"), "{title}" }
            p { style: meta_style, "{meta}" }
        }
    }
}

#[component]
fn ShortcutRow(keys: String, label: String) -> Element {
    let row_style = format!(
        "display: flex; align-items: center; justify-content: space-between; gap: {SPACE_3}; padding: {SPACE_2} 0; border-bottom: 1px solid rgba(143, 181, 255, 0.08);"
    );

    rsx! {
        div {
            style: row_style,
            span { style: "color: #eff5ff;", "{label}" }
            code { style: format!("font-size: {TYPE_CAPTION}; color: #8bb6ff;"), "{keys}" }
        }
    }
}

#[component]
fn InfoBlock(title: String, detail: String) -> Element {
    let block_style = format!(
        "padding: {SPACE_4}; border-radius: 16px; background: rgba(255, 255, 255, 0.03); border: 1px solid rgba(143, 181, 255, 0.08);"
    );
    let detail_style = format!(
        "margin: 6px 0 0; font-size: {TYPE_CAPTION}; color: #91a4c8; line-height: {LEADING_COPY};"
    );

    rsx! {
        div {
            style: block_style,
            h3 { style: format!("margin: 0; font-size: {TYPE_BODY}; line-height: {LEADING_TIGHT};"), "{title}" }
            p { style: detail_style, "{detail}" }
        }
    }
}

#[component]
fn Tab(label: String, active: bool) -> Element {
    let style = if active {
        format!(
            "border-radius: 999px; padding: {SPACE_2} {SPACE_4}; border: 1px solid rgba(143, 181, 255, 0.24); background: rgba(55, 104, 184, 0.28); color: #f4f8ff; font: inherit;"
        )
    } else {
        format!(
            "border-radius: 999px; padding: {SPACE_2} {SPACE_4}; border: 1px solid rgba(143, 181, 255, 0.08); background: rgba(255, 255, 255, 0.03); color: #9cb1d3; font: inherit;"
        )
    };

    rsx! {
        button {
            style: style,
            "{label}"
        }
    }
}

#[component]
fn StatusPill(label: String, tone: String) -> Element {
    let style = format!(
        "display: inline-flex; align-items: center; padding: {SPACE_1} {SPACE_3}; border-radius: 999px; background: color-mix(in srgb, {tone} 18%, transparent); color: #f4f8ff; border: 1px solid color-mix(in srgb, {tone} 32%, transparent); font-size: {TYPE_CAPTION};"
    );

    rsx! {
        span {
            style: style,
            "{label}"
        }
    }
}
