//! Ops workbench state adapter — bridges daemon state to UI clients.
//!
//! Builds [`ProjectOpsSnapshot`] from [`SharedState`], aggregating sessions,
//! history, genome, artifacts, and context health into the schema consumed by
//! UI clients (ratatui TUI and the Dioxus Desktop host). Also provides
//! [`TerminalOpsTelemetryStore`] for collecting and expiring per-terminal
//! operation reports.

use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use impulse_ops::{
    self, AgentRuntime, ArtifactAction, ArtifactActionResult, ArtifactEnvelope, ArtifactFileRef,
    ArtifactStatus, ArtifactViewHint, ContextHealthSummary, InsightRecord,
    InterventionRecommendation, MemorySummary, OpsEvent, OpsSubscription, ProjectOpsSnapshot,
    ProjectSummary, RetrievalSummary, TerminalOpsReport,
};
use serde::Deserialize;
use serde_json::json;

use crate::state::{HistoryEntry, Session, SessionStatus, SharedState};

#[derive(Debug, Deserialize, Default)]
struct GenomeFile {
    #[serde(default)]
    decisions: Vec<GenomeDecision>,
    #[serde(default)]
    last_updated: Option<String>,
}

/// Fields populated via serde deserialization from GENOME.md JSON.
#[derive(Debug, Deserialize, Default)]
// dead_code: fields populated by serde deserialization, not accessed directly in Rust code
#[allow(dead_code)]
struct GenomeDecision {
    #[serde(default, alias = "date")]
    timestamp: Option<String>,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LiveInsightLine {
    #[serde(default)]
    timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    agent_kind: Option<String>,
    #[serde(default)]
    insight_type: Option<String>,
    #[serde(default)]
    content: String,
}

const TELEMETRY_STALE_AFTER_SECS: i64 = 10;
const TELEMETRY_PURGE_AFTER_SECS: i64 = 60;

#[derive(Debug, Clone)]
struct TerminalOpsRecord {
    report: TerminalOpsReport,
    received_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct TerminalOpsTelemetryStore {
    projects: HashMap<String, HashMap<String, TerminalOpsRecord>>,
}

impl TerminalOpsTelemetryStore {
    pub fn publish(&mut self, project_id: &str, report: TerminalOpsReport) {
        let received_at = parse_report_timestamp(&report).unwrap_or_else(Utc::now);
        self.projects
            .entry(project_id.to_string())
            .or_default()
            .insert(
                report.source_id.clone(),
                TerminalOpsRecord {
                    report,
                    received_at,
                },
            );
        self.purge_expired(Utc::now());
    }

    pub fn fresh_reports(
        &mut self,
        project_id: &str,
        now: DateTime<Utc>,
    ) -> Vec<TerminalOpsReport> {
        self.purge_expired(now);
        self.projects
            .get(project_id)
            .map(|records| {
                records
                    .values()
                    .filter(|record| {
                        now - record.received_at
                            <= ChronoDuration::seconds(TELEMETRY_STALE_AFTER_SECS)
                    })
                    .map(|record| record.report.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn purge_expired(&mut self, now: DateTime<Utc>) {
        self.projects.retain(|_, records| {
            records.retain(|_, record| {
                now - record.received_at <= ChronoDuration::seconds(TELEMETRY_PURGE_AFTER_SECS)
            });
            !records.is_empty()
        });
    }
}

pub async fn build_snapshot(
    state: &SharedState,
    terminal_reports: &[TerminalOpsReport],
) -> Result<ProjectOpsSnapshot> {
    let project = project_summary(state);
    sync_legacy_artifacts(state.storage().base_path(), &project)
        .context("Failed to sync legacy artifacts for workbench")?;

    let sessions = state
        .list_sessions()
        .await
        .context("Failed to list sessions for workbench snapshot")?;
    let history = load_history(state.storage().base_path())
        .context("Failed to read history for workbench")?;
    let genome =
        load_genome(state.storage().base_path()).context("Failed to load genome for workbench")?;
    let artifacts = list_artifacts(state.storage().base_path(), &project.id)
        .context("Failed to list artifacts for workbench")?;
    let recent_insights = load_live_insights(state.storage().base_path(), 20)
        .context("Failed to load live insights for workbench")?;
    let pending_review_count = artifacts
        .iter()
        .filter(|artifact| artifact.status == ArtifactStatus::Staged)
        .count();

    let agents = sessions
        .iter()
        .map(|session| build_agent_runtime(session, pending_review_count))
        .collect::<Vec<_>>();
    let interventions = build_interventions(&sessions, &artifacts, &recent_insights);

    let mut snapshot = ProjectOpsSnapshot {
        generated_at: impulse_ops::now_rfc3339(),
        project,
        agents,
        interventions,
        context: ContextHealthSummary {
            tier: if pending_review_count > 0 {
                "review_pending".to_string()
            } else {
                "steady".to_string()
            },
            pending_review_count,
            recent_insights,
            ..Default::default()
        },
        memory: MemorySummary {
            active_sessions: sessions.len(),
            history_entries: history.len(),
            genome_decisions: genome.decisions.len(),
            last_genome_update: genome.last_updated,
        },
        retrieval: build_retrieval_summary(state)
            .context("Failed to build retrieval summary for workbench")?,
        artifacts,
        delegations: Vec::new(),
    };
    overlay_terminal_reports(&mut snapshot, terminal_reports);

    Ok(snapshot)
}

pub async fn subscribe_ops(
    state: &SharedState,
    since_seq: Option<u64>,
    terminal_reports: &[TerminalOpsReport],
) -> Result<OpsSubscription> {
    let snapshot = build_snapshot(state, terminal_reports)
        .await
        .context("Failed to build ops snapshot for subscription")?;
    let next_seq = snapshot_seq(&snapshot);
    let events = if since_seq == Some(next_seq) {
        Vec::new()
    } else {
        build_events(&snapshot, next_seq)
    };

    Ok(OpsSubscription {
        snapshot,
        events,
        next_seq,
    })
}

pub fn list_artifacts(base_path: &Path, project_id: &str) -> Result<Vec<ArtifactEnvelope>> {
    let root = impulse_ops::artifact_store_root(base_path, project_id);
    let mut artifacts = Vec::new();
    if !root.exists() {
        return Ok(artifacts);
    }

    for agent_dir in fs::read_dir(&root)
        .with_context(|| format!("Failed to read artifact store root {}", root.display()))?
    {
        let agent_dir =
            agent_dir.context("Failed to read agent directory entry in artifact store")?;
        let artifact_dir = agent_dir.path().join("artifacts");
        if !artifact_dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&artifact_dir).with_context(|| {
            format!(
                "Failed to read artifact directory {}",
                artifact_dir.display()
            )
        })? {
            let entry = entry.context("Failed to read artifact directory entry")?;
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read(entry.path()).with_context(|| {
                format!("Failed to read artifact file {}", entry.path().display())
            })?;
            let artifact =
                serde_json::from_slice::<ArtifactEnvelope>(&content).with_context(|| {
                    format!(
                        "Failed to parse artifact JSON from {}",
                        entry.path().display()
                    )
                })?;
            artifacts.push(artifact);
        }
    }

    artifacts.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(artifacts)
}

pub fn get_artifact(
    base_path: &Path,
    project_id: &str,
    artifact_id: &str,
) -> Result<Option<ArtifactEnvelope>> {
    let artifacts =
        list_artifacts(base_path, project_id).context("Failed to list artifacts for lookup")?;
    Ok(artifacts
        .into_iter()
        .find(|artifact| artifact.id == artifact_id))
}

pub fn run_artifact_action(
    base_path: &Path,
    project_id: &str,
    artifact_id: &str,
    action_id: &str,
    _params: &serde_json::Value,
) -> Result<ArtifactActionResult> {
    let Some(mut artifact) = get_artifact(base_path, project_id, artifact_id)
        .context("Failed to retrieve artifact for action")?
    else {
        anyhow::bail!("Artifact not found: {}", artifact_id);
    };

    match action_id {
        "review" => Ok(ArtifactActionResult {
            status: "ready".to_string(),
            message: "Artifact loaded for review".to_string(),
            payload: Some(artifact.payload.clone()),
            artifact: Some(artifact),
        }),
        "acknowledge" => {
            artifact.status = ArtifactStatus::Acknowledged;
            impulse_ops::save_artifact(base_path, &artifact)
                .context("Failed to save acknowledged artifact")?;
            Ok(ArtifactActionResult {
                status: "acknowledged".to_string(),
                message: "Artifact acknowledged".to_string(),
                payload: None,
                artifact: Some(artifact),
            })
        }
        "apply" => {
            artifact.status = ArtifactStatus::Applied;
            impulse_ops::save_artifact(base_path, &artifact)
                .context("Failed to save applied artifact")?;
            Ok(ArtifactActionResult {
                status: "ready_to_apply".to_string(),
                message: "Artifact content prepared for the active agent".to_string(),
                payload: Some(json!({
                    "content": artifact.payload.get("markdown").or_else(|| artifact.payload.get("content")).cloned().unwrap_or(serde_json::Value::Null),
                    "related_files": artifact.related_files,
                })),
                artifact: Some(artifact),
            })
        }
        "open_file" => Ok(ArtifactActionResult {
            status: "ready".to_string(),
            message: "Related file located".to_string(),
            payload: Some(json!({
                "path": artifact.related_files.first().map(|file| file.path.clone()),
            })),
            artifact: Some(artifact),
        }),
        other => anyhow::bail!("Unsupported artifact action: {}", other),
    }
}

pub fn project_summary(state: &SharedState) -> ProjectSummary {
    let impulse_path = state.storage().base_path().to_path_buf();
    let project_root = impulse_path
        .parent()
        .unwrap_or_else(|| state.storage().base_path());
    let name = project_root
        .file_name()
        .map(|segment| segment.to_string_lossy().to_string())
        .unwrap_or_else(|| "Impulse Project".to_string());

    ProjectSummary {
        id: impulse_ops::sanitize_id(&name),
        name,
        root_path: project_root.display().to_string(),
        impulse_path: impulse_path.display().to_string(),
    }
}

fn build_agent_runtime(
    session: &Session,
    pending_review_count: usize,
) -> impulse_ops::AgentRuntime {
    let backend_kind = session
        .platform
        .map(|platform| platform.as_str().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let status = match session.status {
        SessionStatus::Active => "active",
        SessionStatus::Idle => "idle",
        SessionStatus::Waiting => "waiting",
        SessionStatus::Completed => "completed",
        SessionStatus::Error => "error",
    };

    let mut warnings = Vec::new();
    if session.active_files.len() >= 6 {
        warnings.push("High file churn across this session".to_string());
    }
    if pending_review_count > 0 {
        warnings.push(format!(
            "{} staged review artifact(s) need attention",
            pending_review_count
        ));
    }
    if matches!(
        session.status,
        SessionStatus::Error | SessionStatus::Waiting
    ) {
        warnings.push(format!("Session is currently {}", status));
    }

    impulse_ops::AgentRuntime {
        id: session.id.clone(),
        label: session.name.clone(),
        backend_kind,
        session_id: Some(session.id.clone()),
        ephemeral: false,
        working_directory: session.working_directory.clone(),
        status: status.to_string(),
        current_task: session.metadata.get("current_task").cloned(),
        active: session.status == SessionStatus::Active,
        context: ContextHealthSummary {
            tier: if pending_review_count > 0 {
                "review_pending".to_string()
            } else {
                "steady".to_string()
            },
            pending_review_count,
            ..Default::default()
        },
        recent_files: session.active_files.clone(),
        recent_tools: session.recent_tools.clone(),
        warnings,
        agent_status: Default::default(),
        role: session.role.clone(),
        group: None,
        tool_invocations: Vec::new(),
        diff_summary: None,
        target: session.target.clone(),
    }
}

fn build_interventions(
    sessions: &[Session],
    artifacts: &[ArtifactEnvelope],
    insights: &[InsightRecord],
) -> Vec<InterventionRecommendation> {
    let mut interventions = Vec::new();

    let staged_artifacts = artifacts
        .iter()
        .filter(|artifact| artifact.status == ArtifactStatus::Staged)
        .count();
    if staged_artifacts > 0 {
        interventions.push(InterventionRecommendation {
            id: "review-staged-artifacts".to_string(),
            title: "Review staged artifacts".to_string(),
            description: format!(
                "{} staged artifact(s) are waiting for review before injection or handoff.",
                staged_artifacts
            ),
            severity: "warning".to_string(),
            action_kind: "review_artifacts".to_string(),
            action_label: "Open Artifacts".to_string(),
            target_agent_id: None,
        });
    }

    for session in sessions {
        if matches!(
            session.status,
            SessionStatus::Waiting | SessionStatus::Error
        ) {
            interventions.push(InterventionRecommendation {
                id: format!("inspect-{}", session.id),
                title: format!("Inspect {}", session.name),
                description: format!(
                    "{} is currently {} and may need operator intervention.",
                    session.name,
                    match session.status {
                        SessionStatus::Waiting => "waiting",
                        SessionStatus::Error => "in error",
                        _ => "paused",
                    }
                ),
                severity: "urgent".to_string(),
                action_kind: "focus_agent".to_string(),
                action_label: "Focus Agent".to_string(),
                target_agent_id: Some(session.id.clone()),
            });
        }
    }

    let recent_errors = insights
        .iter()
        .filter(|insight| insight.kind.eq_ignore_ascii_case("error_encountered"))
        .count();
    if recent_errors > 0 {
        interventions.push(InterventionRecommendation {
            id: "recent-errors".to_string(),
            title: "Recent agent errors detected".to_string(),
            description: format!(
                "{} recent error insight(s) were captured in live agent output.",
                recent_errors
            ),
            severity: "urgent".to_string(),
            action_kind: "review_context".to_string(),
            action_label: "Open Context".to_string(),
            target_agent_id: None,
        });
    }

    interventions
}

fn overlay_terminal_reports(
    snapshot: &mut ProjectOpsSnapshot,
    terminal_reports: &[TerminalOpsReport],
) {
    if terminal_reports.is_empty() {
        return;
    }

    let mut telemetry_interventions = Vec::new();
    let mut telemetry_contexts = Vec::new();

    for report in terminal_reports {
        telemetry_contexts.push(report.context.clone());
        telemetry_interventions.extend(report.interventions.clone());

        for agent in &report.agents {
            if let Some(index) = snapshot.agents.iter().position(|candidate| {
                match (&agent.session_id, &candidate.session_id) {
                    (Some(left), Some(right)) if left == right => true,
                    _ => candidate.id == agent.id,
                }
            }) {
                overlay_agent_runtime(&mut snapshot.agents[index], agent);
            } else {
                let mut ephemeral = agent.clone();
                ephemeral.ephemeral = true;
                snapshot.agents.push(ephemeral);
            }
        }
    }

    snapshot.context = merge_context_summary(&snapshot.context, &telemetry_contexts);
    if !telemetry_interventions.is_empty() {
        snapshot.interventions.extend(telemetry_interventions);
        dedupe_interventions(&mut snapshot.interventions);
    }

    snapshot.agents.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.label.cmp(&right.label))
    });
}

fn overlay_agent_runtime(target: &mut AgentRuntime, overlay: &AgentRuntime) {
    if !overlay.label.is_empty() {
        target.label = overlay.label.clone();
    }
    if !overlay.backend_kind.is_empty() {
        target.backend_kind = overlay.backend_kind.clone();
    }
    if let Some(session_id) = &overlay.session_id {
        target.session_id = Some(session_id.clone());
    }
    target.ephemeral = target.ephemeral && overlay.ephemeral;
    if !overlay.working_directory.is_empty() {
        target.working_directory = overlay.working_directory.clone();
    }
    if !overlay.status.is_empty() {
        target.status = overlay.status.clone();
    }
    if overlay.current_task.is_some() {
        target.current_task = overlay.current_task.clone();
    }
    target.active = overlay.active;
    target.context = overlay.context.clone();
    if !overlay.recent_files.is_empty() {
        target.recent_files = overlay.recent_files.clone();
    }
    if !overlay.recent_tools.is_empty() {
        target.recent_tools = overlay.recent_tools.clone();
    }
    target.warnings.extend(overlay.warnings.clone());
    dedupe_strings(&mut target.warnings);
}

fn merge_context_summary(
    durable: &ContextHealthSummary,
    terminal_contexts: &[ContextHealthSummary],
) -> ContextHealthSummary {
    let mut merged = durable.clone();
    let mut combined_insights = merged.recent_insights.clone();

    for context in terminal_contexts {
        if context.usage_fraction >= merged.usage_fraction {
            merged.tier = context.tier.clone();
            merged.usage_fraction = context.usage_fraction;
            merged.estimated_tokens = context.estimated_tokens;
            merged.window_tokens = context.window_tokens;
        } else if tier_rank(&context.tier) > tier_rank(&merged.tier) {
            merged.tier = context.tier.clone();
        }
        merged.compaction_count = merged
            .compaction_count
            .saturating_add(context.compaction_count);
        merged.injection_count = merged
            .injection_count
            .saturating_add(context.injection_count);
        merged.pending_review_count = merged
            .pending_review_count
            .max(context.pending_review_count);
        combined_insights.extend(context.recent_insights.clone());
    }

    dedupe_insights(&mut combined_insights);
    combined_insights.truncate(20);
    merged.recent_insights = combined_insights;
    if merged.pending_review_count > 0 && merged.tier.is_empty() {
        merged.tier = "review_pending".to_string();
    }
    merged
}

fn dedupe_interventions(interventions: &mut Vec<InterventionRecommendation>) {
    let mut seen = HashSet::new();
    interventions.retain(|intervention| seen.insert(intervention.id.clone()));
}

fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn dedupe_insights(insights: &mut Vec<InsightRecord>) {
    let mut seen = HashSet::new();
    insights.retain(|insight| {
        seen.insert(format!(
            "{}|{}|{}|{}",
            insight.timestamp.clone().unwrap_or_default(),
            insight.agent_label,
            insight.kind,
            insight.content
        ))
    });
}

fn tier_rank(tier: &str) -> u8 {
    match tier {
        "minimal" => 4,
        "critical" => 3,
        "review_pending" => 2,
        "essential" => 1,
        _ => 0,
    }
}

fn parse_report_timestamp(report: &TerminalOpsReport) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&report.published_at)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn snapshot_seq(snapshot: &ProjectOpsSnapshot) -> u64 {
    let mut stable = snapshot.clone();
    stable.generated_at.clear();
    let serialized = serde_json::to_string(&stable).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    hasher.finish()
}

fn build_events(snapshot: &ProjectOpsSnapshot, seq: u64) -> Vec<OpsEvent> {
    let mut events = Vec::new();

    for (offset, intervention) in snapshot.interventions.iter().take(5).enumerate() {
        events.push(OpsEvent {
            seq: seq.saturating_sub(offset as u64),
            kind: intervention.action_kind.clone(),
            severity: intervention.severity.clone(),
            title: intervention.title.clone(),
            detail: intervention.description.clone(),
            agent_id: intervention.target_agent_id.clone(),
            created_at: snapshot.generated_at.clone(),
        });
    }

    if events.is_empty() {
        events.push(OpsEvent {
            seq,
            kind: "snapshot_refreshed".to_string(),
            severity: "ambient".to_string(),
            title: "Workbench snapshot refreshed".to_string(),
            detail: format!(
                "{} agents, {} artifacts",
                snapshot.agents.len(),
                snapshot.artifacts.len()
            ),
            agent_id: None,
            created_at: snapshot.generated_at.clone(),
        });
    }

    events
}

fn build_retrieval_summary(state: &SharedState) -> Result<RetrievalSummary> {
    let config = state
        .config_snapshot()
        .context("Failed to read config snapshot for retrieval summary")?;
    Ok(RetrievalSummary {
        mode: config.retrieval_mode,
        backend: config.retrieval_backend,
        vector_enabled: config.retrieval_vector_enabled,
        semantic_strategy: config.retrieval_semantic_strategy,
    })
}

fn load_history(base_path: &Path) -> Result<Vec<HistoryEntry>> {
    crate::storage::Storage::new(base_path.to_path_buf()).read_jsonl("HISTORY.jsonl")
}

fn load_genome(base_path: &Path) -> Result<GenomeFile> {
    let genome_path = base_path.join("GENOME.md");
    if !genome_path.exists() {
        return Ok(GenomeFile::default());
    }
    let content = fs::read_to_string(&genome_path)
        .with_context(|| format!("Failed to read genome file {}", genome_path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse genome file {}", genome_path.display()))
}

fn load_live_insights(base_path: &Path, limit: usize) -> Result<Vec<InsightRecord>> {
    let path = base_path.join("LIVE_INSIGHTS.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut insights = Vec::new();
    for line in fs::read_to_string(&path)
        .context("Failed to read live insights file")?
        .lines()
    {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<LiveInsightLine>(line) else {
            continue;
        };
        insights.push(InsightRecord {
            timestamp: parsed.timestamp.map(|value| value.to_rfc3339()),
            agent_label: parsed
                .agent_kind
                .unwrap_or_else(|| "unknown".to_string())
                .replace('_', " "),
            kind: parsed.insight_type.unwrap_or_else(|| "unknown".to_string()),
            content: parsed.content,
        });
    }

    insights.reverse();
    insights.truncate(limit);
    Ok(insights)
}

fn sync_legacy_artifacts(base_path: &Path, project: &ProjectSummary) -> Result<()> {
    sync_legacy_injection_artifacts(base_path, project)
        .context("Failed to sync legacy injection artifacts")?;
    sync_live_insights_artifact(base_path, project)
        .context("Failed to sync live insights artifact")?;
    Ok(())
}

fn sync_legacy_injection_artifacts(base_path: &Path, project: &ProjectSummary) -> Result<()> {
    let injections_dir = base_path.join("context").join("injections");
    if !injections_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&injections_dir).with_context(|| {
        format!(
            "Failed to read injections directory {}",
            injections_dir.display()
        )
    })? {
        let entry = entry.context("Failed to read injection directory entry")?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read injection file {}", path.display()))?;
        let stem = path
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "injection".to_string());
        let artifact = ArtifactEnvelope {
            id: impulse_ops::sanitize_id(&format!("legacy-{}", stem)),
            project_id: project.id.clone(),
            agent_id: "system-injection".to_string(),
            session_id: None,
            kind: "injection_review".to_string(),
            schema: "impulse.injection.markdown/v1".to_string(),
            title: format!("Injection Review: {}", stem),
            summary: summarize_markdown(&content),
            payload: json!({
                "markdown": content,
                "source_path": path.display().to_string(),
            }),
            view_hints: vec![
                ArtifactViewHint::SummaryCard,
                ArtifactViewHint::Markdown,
                ArtifactViewHint::RawJson,
            ],
            actions: vec![
                action("review", "Review", "review", false),
                action("apply", "Apply To Active Agent", "apply", true),
                action("acknowledge", "Acknowledge", "acknowledge", false),
                action("open_file", "Open File Ref", "open_file", false),
            ],
            status: ArtifactStatus::Staged,
            created_at: impulse_ops::file_modified_to_rfc3339(&path),
            related_files: vec![ArtifactFileRef {
                path: path.display().to_string(),
                label: Some("Legacy injection artifact".to_string()),
            }],
            metadata: json!({
                "source": "context/injections",
            }),
        };
        impulse_ops::save_artifact(base_path, &artifact)
            .with_context(|| format!("Failed to save legacy injection artifact {}", stem))?;
    }
    Ok(())
}

fn sync_live_insights_artifact(base_path: &Path, project: &ProjectSummary) -> Result<()> {
    let live_insights = load_live_insights(base_path, 50)
        .context("Failed to load live insights for artifact sync")?;
    if live_insights.is_empty() {
        return Ok(());
    }

    let artifact = ArtifactEnvelope {
        id: "live-insights".to_string(),
        project_id: project.id.clone(),
        agent_id: "workspace".to_string(),
        session_id: None,
        kind: "insight_timeline".to_string(),
        schema: "impulse.live_insights.timeline/v1".to_string(),
        title: "Recent Live Insights".to_string(),
        summary: format!(
            "{} recent insight(s) captured from active GUI terminals.",
            live_insights.len()
        ),
        payload: json!({
            "entries": live_insights,
        }),
        view_hints: vec![
            ArtifactViewHint::SummaryCard,
            ArtifactViewHint::Timeline,
            ArtifactViewHint::Table,
            ArtifactViewHint::RawJson,
        ],
        actions: vec![
            action("review", "Review", "review", false),
            action("acknowledge", "Acknowledge", "acknowledge", false),
        ],
        status: ArtifactStatus::Ready,
        created_at: impulse_ops::now_rfc3339(),
        related_files: vec![ArtifactFileRef {
            path: base_path.join("LIVE_INSIGHTS.jsonl").display().to_string(),
            label: Some("Live insights log".to_string()),
        }],
        metadata: json!({
            "source": "LIVE_INSIGHTS.jsonl",
        }),
    };
    impulse_ops::save_artifact(base_path, &artifact)
        .context("Failed to save live insights artifact")?;
    Ok(())
}

fn summarize_markdown(content: &str) -> String {
    content
        .lines()
        .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "No summary available.".to_string())
}

fn action(id: &str, label: &str, kind: &str, requires_confirmation: bool) -> ArtifactAction {
    ArtifactAction {
        id: id.to_string(),
        label: label.to_string(),
        kind: kind.to_string(),
        requires_confirmation,
        params_schema: serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_build_snapshot_includes_mirrored_legacy_artifacts() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("context/injections")).unwrap();
        std::fs::write(
            temp.path().join("context/injections/inject-001.md"),
            "# Injection Bundle\n\n- Query: auth\n",
        )
        .unwrap();

        let state = crate::state::State::new(temp.path().to_path_buf()).unwrap();
        let shared = std::sync::Arc::new(state);
        let snapshot = build_snapshot(&shared, &[]).await.unwrap();

        assert!(!snapshot.artifacts.is_empty());
        assert!(snapshot
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == "injection_review"));
    }

    #[tokio::test]
    async fn test_build_snapshot_overlays_terminal_reports() {
        let temp = TempDir::new().unwrap();
        let state = crate::state::State::new(temp.path().to_path_buf()).unwrap();
        let shared = std::sync::Arc::new(state);

        let report = TerminalOpsReport {
            source_id: "gui-test".to_string(),
            published_at: impulse_ops::now_rfc3339(),
            agents: vec![AgentRuntime {
                id: "tab-1".to_string(),
                label: "Claude Code: demo".to_string(),
                backend_kind: "Claude Code".to_string(),
                session_id: None,
                ephemeral: true,
                working_directory: temp.path().display().to_string(),
                status: "active".to_string(),
                current_task: Some("Investigating telemetry".to_string()),
                active: true,
                context: ContextHealthSummary {
                    tier: "critical".to_string(),
                    usage_fraction: 0.72,
                    estimated_tokens: 72_000,
                    window_tokens: 100_000,
                    compaction_count: 2,
                    injection_count: 1,
                    pending_review_count: 1,
                    recent_insights: vec![InsightRecord {
                        timestamp: Some(impulse_ops::now_rfc3339()),
                        agent_label: "Claude Code".to_string(),
                        kind: "decision_made".to_string(),
                        content: "Adopt daemon-truth overlay".to_string(),
                    }],
                },
                recent_files: vec!["src/app.rs".to_string()],
                recent_tools: vec!["Edit".to_string()],
                warnings: vec!["Context tier is critical".to_string()],
                ..Default::default()
            }],
            context: ContextHealthSummary {
                tier: "critical".to_string(),
                usage_fraction: 0.72,
                estimated_tokens: 72_000,
                window_tokens: 100_000,
                compaction_count: 2,
                injection_count: 1,
                pending_review_count: 1,
                recent_insights: vec![InsightRecord {
                    timestamp: Some(impulse_ops::now_rfc3339()),
                    agent_label: "Claude Code".to_string(),
                    kind: "decision_made".to_string(),
                    content: "Adopt daemon-truth overlay".to_string(),
                }],
            },
            interventions: vec![InterventionRecommendation {
                id: "focus-tab-1".to_string(),
                title: "Focus Claude Code: demo".to_string(),
                description: "Context is at a critical tier.".to_string(),
                severity: "warning".to_string(),
                action_kind: "focus_agent".to_string(),
                action_label: "Focus Agent".to_string(),
                target_agent_id: Some("tab-1".to_string()),
            }],
        };

        let snapshot = build_snapshot(&shared, &[report]).await.unwrap();

        assert_eq!(snapshot.agents.len(), 1);
        assert_eq!(snapshot.agents[0].id, "tab-1");
        assert!(snapshot.agents[0].ephemeral);
        assert_eq!(snapshot.context.tier, "critical");
        assert_eq!(snapshot.context.compaction_count, 2);
        assert!(snapshot
            .interventions
            .iter()
            .any(|item| item.id == "focus-tab-1"));
    }

    #[tokio::test]
    async fn test_build_snapshot_matches_session_id_before_agent_id() {
        let temp = TempDir::new().unwrap();
        let state = crate::state::State::new(temp.path().to_path_buf()).unwrap();
        let shared = std::sync::Arc::new(state);
        let session = shared
            .create_session("daemon-session".to_string(), None)
            .await
            .unwrap();

        let report = TerminalOpsReport {
            source_id: "gui-test".to_string(),
            published_at: impulse_ops::now_rfc3339(),
            agents: vec![AgentRuntime {
                id: "tab-99".to_string(),
                label: "Daemon Session".to_string(),
                backend_kind: "Claude Code".to_string(),
                session_id: Some(session.id.clone()),
                ephemeral: true,
                working_directory: temp.path().display().to_string(),
                status: "active".to_string(),
                current_task: Some("Merged by session id".to_string()),
                active: true,
                context: ContextHealthSummary {
                    tier: "essential".to_string(),
                    usage_fraction: 0.5,
                    estimated_tokens: 50_000,
                    window_tokens: 100_000,
                    ..Default::default()
                },
                recent_files: vec!["src/main.rs".to_string()],
                recent_tools: vec!["Write".to_string()],
                warnings: Vec::new(),
                ..Default::default()
            }],
            context: ContextHealthSummary::default(),
            interventions: Vec::new(),
        };

        let snapshot = build_snapshot(&shared, &[report]).await.unwrap();

        assert_eq!(snapshot.agents.len(), 1);
        assert_eq!(snapshot.agents[0].id, session.id);
        assert_eq!(
            snapshot.agents[0].current_task.as_deref(),
            Some("Merged by session id")
        );
        assert!(!snapshot.agents[0].ephemeral);
        assert_eq!(snapshot.agents[0].context.tier, "essential");
    }

    #[tokio::test]
    async fn test_build_snapshot_uses_codex_backend_kind() {
        let temp = TempDir::new().unwrap();
        let state = crate::state::State::new(temp.path().to_path_buf()).unwrap();
        let shared = std::sync::Arc::new(state);
        let session = shared
            .create_session(
                "codex-session".to_string(),
                Some(crate::state::Platform::Codex),
            )
            .await
            .unwrap();

        let snapshot = build_snapshot(&shared, &[]).await.unwrap();
        let agent = snapshot
            .agents
            .iter()
            .find(|agent| agent.session_id.as_deref() == Some(&session.id))
            .expect("codex session appears in ops snapshot");

        assert_eq!(agent.backend_kind, "codex");
    }

    #[tokio::test]
    async fn test_build_snapshot_returns_error_for_malformed_genome() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("GENOME.md"), "{not-json").unwrap();

        let state = crate::state::State::new(temp.path().to_path_buf()).unwrap();
        let shared = std::sync::Arc::new(state);

        let err = build_snapshot(&shared, &[]).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("Failed to parse genome") || msg.contains("Failed to load genome"),
            "expected genome parse/load error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_subscribe_ops_uses_stable_snapshot_sequence() {
        let temp = TempDir::new().unwrap();
        let state = crate::state::State::new(temp.path().to_path_buf()).unwrap();
        let shared = std::sync::Arc::new(state);

        let first = subscribe_ops(&shared, None, &[]).await.unwrap();
        let second = subscribe_ops(&shared, Some(first.next_seq), &[])
            .await
            .unwrap();

        assert_eq!(first.next_seq, second.next_seq);
        assert!(!first.events.is_empty());
        assert!(second.events.is_empty());
    }

    #[test]
    fn test_terminal_ops_store_marks_reports_stale_and_purges_old_entries() {
        let now = Utc::now();
        let mut store = TerminalOpsTelemetryStore::default();
        store.projects.insert(
            "demo".to_string(),
            HashMap::from([
                (
                    "fresh".to_string(),
                    TerminalOpsRecord {
                        report: TerminalOpsReport {
                            source_id: "fresh".to_string(),
                            published_at: now.to_rfc3339(),
                            ..Default::default()
                        },
                        received_at: now,
                    },
                ),
                (
                    "stale".to_string(),
                    TerminalOpsRecord {
                        report: TerminalOpsReport {
                            source_id: "stale".to_string(),
                            published_at: now.to_rfc3339(),
                            ..Default::default()
                        },
                        received_at: now - ChronoDuration::seconds(TELEMETRY_STALE_AFTER_SECS + 1),
                    },
                ),
                (
                    "expired".to_string(),
                    TerminalOpsRecord {
                        report: TerminalOpsReport {
                            source_id: "expired".to_string(),
                            published_at: now.to_rfc3339(),
                            ..Default::default()
                        },
                        received_at: now - ChronoDuration::seconds(TELEMETRY_PURGE_AFTER_SECS + 1),
                    },
                ),
            ]),
        );

        let reports = store.fresh_reports("demo", now);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].source_id, "fresh");
        assert!(!store.projects["demo"].contains_key("expired"));
        assert!(store.projects["demo"].contains_key("stale"));
    }
}
