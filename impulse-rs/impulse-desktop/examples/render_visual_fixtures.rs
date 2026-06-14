use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use dioxus::prelude::*;
use impulse_desktop::{
    default_builtin_mcp_tools, AgentPlatformKind, AgentRuntimeSnapshot, DesktopShellWithSnapshot,
    DesktopShellWithSnapshotProps, DesktopView, McpInvocation, ReviewQueueItem, ReviewQueueStatus,
    WorkspaceEntry, WorkspaceTarget,
};
use impulse_ops::{
    AgentRole, AgentRuntime, AgentStatus, ArtifactAction, ArtifactEnvelope, ArtifactFileRef,
    ArtifactStatus, ArtifactViewHint, ContextHealthSummary, DelegationSummary, DiffSummary,
    InsightRecord, InterventionRecommendation, MachineTarget, MemorySummary, ProjectOpsSnapshot,
    RetrievalSummary, ToolInvocationRecord,
};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../output/playwright/impulse-desktop-visual"));
    fs::create_dir_all(&output_dir)?;

    let snapshot = seeded_snapshot();
    let runtime_agents = seeded_runtime_agents();
    let workspaces = seeded_workspaces();
    let mcp_tools = default_builtin_mcp_tools();
    let last_invocations = seeded_invocations();
    let review_queue = seeded_review_queue();

    for view in DesktopView::ALL {
        let mut vdom = VirtualDom::new_with_props(
            DesktopShellWithSnapshot,
            DesktopShellWithSnapshotProps {
                snapshot: snapshot.clone(),
                runtime_agents: runtime_agents.clone(),
                workspaces: workspaces.clone(),
                mcp_tools: mcp_tools.clone(),
                last_invocations: last_invocations.clone(),
                review_queue: review_queue.clone(),
                initial_view: view,
            },
        );
        vdom.rebuild_in_place();
        let body = dioxus_ssr::render(&vdom);
        let html = wrap_html(view, &body, &asset_base_href()?);
        let path = output_dir.join(format!("{}.html", view.slug()));
        fs::write(&path, html)?;
        println!("{}", path.display());
    }

    Ok(())
}

fn wrap_html(view: DesktopView, body: &str, asset_base_href: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <base href="{asset_base_href}">
    <title>Impulse Desktop {}</title>
  </head>
  <body data-fixture-route="{}">
{}
  </body>
</html>
"#,
        view.label(),
        view.slug(),
        body
    )
}

fn asset_base_href() -> Result<String, Box<dyn std::error::Error>> {
    let mut base = path_to_file_url(&env::current_dir()?)?;
    if !base.ends_with('/') {
        base.push('/');
    }
    Ok(base)
}

fn path_to_file_url(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let absolute = path.canonicalize()?;
    let raw = absolute.to_string_lossy();
    let mut url = String::from("file://");
    for byte in raw.as_bytes() {
        match *byte {
            b' ' => url.push_str("%20"),
            b'#' => url.push_str("%23"),
            b'%' => url.push_str("%25"),
            b'?' => url.push_str("%3F"),
            b'\\' => url.push('/'),
            value => url.push(value as char),
        }
    }
    Ok(url)
}

fn seeded_snapshot() -> ProjectOpsSnapshot {
    let context = ContextHealthSummary {
        tier: "operator".to_string(),
        usage_fraction: 0.42,
        estimated_tokens: 83_740,
        window_tokens: 200_000,
        compaction_count: 3,
        injection_count: 11,
        pending_review_count: 2,
        recent_insights: vec![
            InsightRecord {
                timestamp: Some("2026-06-13T15:20:00Z".to_string()),
                agent_label: "Codex".to_string(),
                kind: "architecture".to_string(),
                content: "Dioxus owns view state; Rust owns terminal truth.".to_string(),
            },
            InsightRecord {
                timestamp: Some("2026-06-13T15:25:00Z".to_string()),
                agent_label: "Claude".to_string(),
                kind: "review".to_string(),
                content: "Keep xterm mounted while routing non-terminal views.".to_string(),
            },
        ],
    };
    ProjectOpsSnapshot {
        generated_at: "2026-06-13T15:30:00Z".to_string(),
        agents: vec![
            AgentRuntime {
                id: "codex-live".to_string(),
                label: "Codex Live".to_string(),
                backend_kind: "pty".to_string(),
                session_id: Some("codex-live-session".to_string()),
                working_directory: "<repo>".to_string(),
                status: "working".to_string(),
                current_task: Some(
                    "visual smoke fixture with a deliberately long task label".to_string(),
                ),
                active: true,
                context: context.clone(),
                recent_files: vec!["impulse-desktop/src/ui.rs".to_string()],
                recent_tools: vec!["impulse.agent_spawn".to_string()],
                warnings: vec![],
                agent_status: AgentStatus::Working {
                    task: "build visual fixture".to_string(),
                },
                role: Some(AgentRole::Coordinator),
                group: Some("desktop".to_string()),
                tool_invocations: vec![ToolInvocationRecord {
                    kind: "mcp".to_string(),
                    target: "impulse.agent_spawn".to_string(),
                    timestamp: Some("2026-06-13T15:12:00Z".to_string()),
                }],
                diff_summary: Some(DiffSummary {
                    files_changed: 6,
                    lines_added: 240,
                    lines_removed: 8,
                }),
                target: Some(MachineTarget::Local {
                    workdir: "<repo>".to_string(),
                }),
                ephemeral: false,
            },
            AgentRuntime {
                id: "claude-review".to_string(),
                label: "Claude Review".to_string(),
                backend_kind: "pty".to_string(),
                working_directory: "<repo>/.worktrees/claude-desktop-views".to_string(),
                status: "blocked".to_string(),
                current_task: Some("waiting for merge owner".to_string()),
                active: true,
                agent_status: AgentStatus::Blocked {
                    reason: "merge window held".to_string(),
                },
                role: Some(AgentRole::Worker { parent_pane_id: 1 }),
                group: Some("review".to_string()),
                ..Default::default()
            },
        ],
        context,
        memory: MemorySummary {
            active_sessions: 4,
            history_entries: 128,
            genome_decisions: 19,
            last_genome_update: Some("2026-06-13".to_string()),
        },
        retrieval: RetrievalSummary {
            mode: "hybrid".to_string(),
            backend: "sqlite-vector".to_string(),
            vector_enabled: true,
            semantic_strategy: "tiered-memory-context".to_string(),
        },
        interventions: vec![InterventionRecommendation {
            id: "iv-package-xterm".to_string(),
            title: "Bundle xterm assets before packaged desktop release".to_string(),
            description:
                "The visual fixture proves layout, not runtime terminal JS asset delivery."
                    .to_string(),
            severity: "warn".to_string(),
            action_kind: "packaging".to_string(),
            action_label: "track".to_string(),
            target_agent_id: Some("codex-live".to_string()),
        }],
        artifacts: seeded_artifacts(),
        delegations: vec![DelegationSummary {
            id: "del-visual".to_string(),
            task: "Render static Dioxus route fixtures and inspect browser layout".to_string(),
            state: "working".to_string(),
            coordinator_pane_id: 1,
            worker_pane_id: Some(2),
            created_at: "2026-06-13T15:00:00Z".to_string(),
            completed_at: None,
            tool_invocations: vec![ToolInvocationRecord {
                kind: "playwright".to_string(),
                target: "chromium".to_string(),
                timestamp: Some("2026-06-13T15:28:00Z".to_string()),
            }],
            diff_summary: Some(DiffSummary {
                files_changed: 5,
                lines_added: 210,
                lines_removed: 4,
            }),
        }],
        ..Default::default()
    }
}

fn seeded_artifacts() -> Vec<ArtifactEnvelope> {
    vec![
        ArtifactEnvelope {
            id: "artifact-phase-f".to_string(),
            project_id: "impulse-rs".to_string(),
            agent_id: "codex-live".to_string(),
            session_id: Some("codex-live-session".to_string()),
            kind: "visual_report".to_string(),
            schema: "impulse.artifact.visual_report.v1".to_string(),
            title: "Phase F visual smoke report".to_string(),
            summary:
                "Static route fixtures for terminal, memory, review, artifacts, and supervisor."
                    .to_string(),
            payload: json!({ "routes": 5, "viewports": ["1440x900", "1024x768"] }),
            view_hints: vec![ArtifactViewHint::SummaryCard, ArtifactViewHint::Log],
            actions: vec![ArtifactAction {
                id: "open-report".to_string(),
                label: "open".to_string(),
                kind: "open_file".to_string(),
                requires_confirmation: false,
                params_schema: json!({ "type": "object" }),
            }],
            status: ArtifactStatus::Pending,
            created_at: "2026-06-13T15:30:00Z".to_string(),
            related_files: vec![ArtifactFileRef {
                path: "output/playwright/impulse-desktop-visual/terminal-1440x900.png".to_string(),
                label: Some("desktop screenshot".to_string()),
            }],
            metadata: json!({ "offline_fonts": true }),
        },
        ArtifactEnvelope {
            id: "artifact-review".to_string(),
            project_id: "impulse-rs".to_string(),
            agent_id: "claude-review".to_string(),
            kind: "merge_plan".to_string(),
            schema: "impulse.artifact.plan.v1".to_string(),
            title: "Desktop view merge plan".to_string(),
            summary: "Keep Codex shell as base and graft view-router components additively."
                .to_string(),
            view_hints: vec![ArtifactViewHint::Markdown],
            status: ArtifactStatus::Applied,
            created_at: "2026-06-13T14:40:00Z".to_string(),
            ..Default::default()
        },
    ]
}

fn seeded_runtime_agents() -> Vec<AgentRuntimeSnapshot> {
    vec![
        runtime_agent("codex-live", "Codex Live", AgentPlatformKind::Codex, true),
        runtime_agent(
            "claude-review",
            "Claude Review",
            AgentPlatformKind::ClaudeCode,
            false,
        ),
    ]
}

fn runtime_agent(
    id: &str,
    label: &str,
    platform: AgentPlatformKind,
    focused: bool,
) -> AgentRuntimeSnapshot {
    let workspace = WorkspaceTarget {
        root: "<repo>".to_string(),
        label: Some("IMPULSE-rs".to_string()),
        purpose: Some("Dioxus terminal harness".to_string()),
        project_notes: Some("Visual smoke uses static SSR fixtures.".to_string()),
    };
    AgentRuntimeSnapshot {
        agent_id: id.to_string(),
        label: label.to_string(),
        platform,
        command: platform.default_command(),
        args: Vec::new(),
        cwd: Some(workspace.root.clone()),
        workspace: Some(workspace),
        session_id: Some(format!("{id}-session")),
        rows: 32,
        cols: 120,
        alive: true,
        focused,
        status: if focused {
            AgentStatus::Working {
                task: "render visual smoke".to_string(),
            }
        } else {
            AgentStatus::Idle
        },
        current_task: Some("visual verification pass".to_string()),
        role: if focused {
            Some(AgentRole::Coordinator)
        } else {
            Some(AgentRole::Worker { parent_pane_id: 1 })
        },
        target: Some(MachineTarget::Local {
            workdir: "<repo>".to_string(),
        }),
        mcp_tools: default_builtin_mcp_tools(),
        output_bytes: 18_240,
        output_lines: 312,
        context: ContextHealthSummary {
            tier: "operator".to_string(),
            usage_fraction: 0.34,
            estimated_tokens: 68_000,
            window_tokens: 200_000,
            ..Default::default()
        },
    }
}

fn seeded_workspaces() -> Vec<WorkspaceEntry> {
    [
        ("<repo>", "IMPULSE-rs", "active Dioxus terminal harness"),
        (
            "/workspace/rosa-example",
            "ROSA Swift",
            "adjacent local-first assistant implementation",
        ),
    ]
    .into_iter()
    .map(|(root, label, purpose)| {
        WorkspaceEntry::new(WorkspaceTarget {
            root: root.to_string(),
            label: Some(label.to_string()),
            purpose: Some(purpose.to_string()),
            project_notes: Some("operator-authored project context is explicit".to_string()),
        })
    })
    .collect()
}

fn seeded_review_queue() -> Vec<ReviewQueueItem> {
    vec![
        ReviewQueueItem {
            id: "review-visual-layout".to_string(),
            staged_at_unix_ms: 1_718_294_200_000,
            status: ReviewQueueStatus::Pending,
            decided_at_unix_ms: None,
            decision: None,
            target_agent_id: Some("codex-live".to_string()),
            arguments: json!({ "content": "Add visual smoke verification notes to memory context." }),
            path: visual_path("review-visual-layout.json"),
            preview: "Add visual smoke verification notes to memory context.".to_string(),
        },
        ReviewQueueItem {
            id: "review-package-xterm".to_string(),
            staged_at_unix_ms: 1_718_294_260_000,
            status: ReviewQueueStatus::Pending,
            decided_at_unix_ms: None,
            decision: None,
            target_agent_id: None,
            arguments: json!({ "content": "Track local xterm bundle follow-up before packaged app release." }),
            path: visual_path("review-package-xterm.json"),
            preview: "Track local xterm bundle follow-up before packaged app release.".to_string(),
        },
    ]
}

fn seeded_invocations() -> Vec<McpInvocation> {
    vec![McpInvocation {
        call_id: "call-agent-spawn".to_string(),
        tool: "impulse.agent_spawn".to_string(),
        caller_agent_id: Some("codex-live".to_string()),
        arguments: json!({ "workspace": "<repo>" }),
        confirmed: true,
        result: json!({ "agent_id": "codex-live", "ok": true }),
        ok: true,
    }]
}

fn visual_path(file_name: &str) -> String {
    Path::new("output")
        .join("playwright")
        .join("impulse-desktop-visual")
        .join(file_name)
        .display()
        .to_string()
}
