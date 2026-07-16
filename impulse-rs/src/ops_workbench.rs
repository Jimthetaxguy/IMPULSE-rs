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
    pub fn publish(&mut self, project_id: &str, mut report: TerminalOpsReport) {
        let received_at = Utc::now();
        let project = self.projects.entry(project_id.to_string()).or_default();
        if let Some(previous) = project.get(&report.source_id) {
            preserve_omitted_same_source_agent_facts(&previous.report, &mut report);
        }
        project.insert(
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

fn preserve_omitted_same_source_agent_facts(
    previous: &TerminalOpsReport,
    incoming: &mut TerminalOpsReport,
) {
    let previous_by_id = previous
        .agents
        .iter()
        .filter(|agent| !agent.id.is_empty())
        .map(|agent| (agent.id.as_str(), agent))
        .collect::<HashMap<_, _>>();

    for agent in &mut incoming.agents {
        if agent.id.is_empty() {
            continue;
        }
        let Some(previous_agent) = previous_by_id.get(agent.id.as_str()) else {
            continue;
        };
        if previous_agent.session_id != agent.session_id {
            continue;
        }
        if agent.current_task.is_none() {
            agent.current_task = previous_agent.current_task.clone();
        }
        if agent.role_assignment.is_none() && agent.role_compatibility.is_none() {
            agent.role_assignment = previous_agent.role_assignment.clone();
            agent.role_compatibility = previous_agent.role_compatibility.clone();
        }
        if agent.governed_task_id.is_none() {
            agent.governed_task_id = previous_agent.governed_task_id.clone();
            agent.governed_task_revision = previous_agent.governed_task_revision;
        } else if agent.governed_task_id == previous_agent.governed_task_id {
            agent.governed_task_revision = match (
                agent.governed_task_revision,
                previous_agent.governed_task_revision,
            ) {
                (Some(incoming), Some(previous)) => Some(incoming.max(previous)),
                (incoming, previous) => incoming.or(previous),
            };
        }
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
    let governed_tasks = state
        .list_governed_tasks(&project.id)
        .context("Failed to list governed tasks for workbench")?;
    let memory_candidates = state
        .list_accepted_run_memory_candidates(&project.id)
        .context("Failed to list accepted-run memory candidates for workbench")?;
    let recent_insights = load_live_insights(state.storage().base_path(), 20)
        .context("Failed to load live insights for workbench")?;
    let pending_review_count = artifacts
        .iter()
        .filter(|artifact| artifact.status == ArtifactStatus::Staged)
        .count()
        .saturating_add(memory_candidates.len());

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
        governed_tasks,
        memory_candidates,
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
        governed_task_id: None,
        governed_task_revision: None,
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
        role_assignment: None,
        role_compatibility: None,
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

    // Structured fields were added after the original telemetry wire shape and
    // deserialize with serde defaults. Until the report carries a schema
    // version or presence markers, a default can mean either "explicitly
    // clear" or "older publisher omitted this field". Preserve durable facts
    // on omission while still letting populated live telemetry take precedence.
    // The legacy status string disambiguates defaulted structured status from
    // explicit lifecycle transitions emitted by older publishers.
    let live_status = if overlay.agent_status != impulse_ops::AgentStatus::Idle {
        Some(overlay.agent_status.clone())
    } else {
        agent_status_from_legacy(&overlay.status)
    };
    if let Some(live_status) = live_status {
        target.agent_status = live_status;
    }
    if overlay.role.is_some() {
        target.role = overlay.role.clone();
    }
    if overlay.role_assignment.is_some() {
        target.role_assignment = overlay.role_assignment.clone();
    }
    if overlay.role_compatibility.is_some() {
        target.role_compatibility = overlay.role_compatibility.clone();
    }
    if overlay.group.is_some() {
        target.group = overlay.group.clone();
    }
    if !overlay.tool_invocations.is_empty() {
        target.tool_invocations = overlay.tool_invocations.clone();
    }
    if overlay.diff_summary.is_some() {
        target.diff_summary = overlay.diff_summary.clone();
    }
    if overlay.target.is_some() {
        target.target = overlay.target.clone();
    }
}

fn agent_status_from_legacy(status: &str) -> Option<impulse_ops::AgentStatus> {
    let status = status.trim();
    if status.eq_ignore_ascii_case("starting") {
        return Some(impulse_ops::AgentStatus::Starting);
    }
    if status.eq_ignore_ascii_case("idle") {
        return Some(impulse_ops::AgentStatus::Idle);
    }
    if status.eq_ignore_ascii_case("interrupted") {
        return Some(impulse_ops::AgentStatus::Interrupted);
    }
    if status.eq_ignore_ascii_case("completed") {
        return Some(impulse_ops::AgentStatus::Completed);
    }

    let (kind, detail) = status.split_once(':')?;
    let detail = detail.trim();
    if kind.trim().eq_ignore_ascii_case("working") {
        return Some(impulse_ops::AgentStatus::Working {
            task: detail.to_string(),
        });
    }
    if kind.trim().eq_ignore_ascii_case("blocked") {
        return Some(impulse_ops::AgentStatus::Blocked {
            reason: detail.to_string(),
        });
    }
    None
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
                governed_task_id: None,
                governed_task_revision: None,
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
                warnings: vec!["live warning".to_string(), "live warning".to_string()],
                agent_status: impulse_ops::AgentStatus::Blocked {
                    reason: "merge conflict in src/main.rs".to_string(),
                },
                role: Some(impulse_ops::AgentRole::Worker { parent_pane_id: 42 }),
                role_assignment: None,
                role_compatibility: None,
                group: Some("review-wave-2".to_string()),
                tool_invocations: vec![impulse_ops::ToolInvocationRecord {
                    kind: "write".to_string(),
                    target: "src/main.rs".to_string(),
                    timestamp: Some("2026-07-11T05:00:00Z".to_string()),
                }],
                diff_summary: Some(impulse_ops::DiffSummary {
                    files_changed: 2,
                    lines_added: 18,
                    lines_removed: 4,
                }),
                target: Some(impulse_ops::MachineTarget::Local {
                    workdir: temp.path().display().to_string(),
                }),
            }],
            context: ContextHealthSummary::default(),
            interventions: Vec::new(),
        };

        let snapshot = build_snapshot(&shared, &[report]).await.unwrap();

        assert_eq!(snapshot.agents.len(), 1);
        assert_eq!(snapshot.agents[0].id, session.id);
        assert_eq!(snapshot.agents[0].label, "Daemon Session");
        assert_eq!(snapshot.agents[0].backend_kind, "Claude Code");
        assert_eq!(snapshot.agents[0].status, "active");
        assert_eq!(
            snapshot.agents[0].current_task.as_deref(),
            Some("Merged by session id")
        );
        assert!(snapshot.agents[0].active);
        assert!(!snapshot.agents[0].ephemeral);
        assert_eq!(snapshot.agents[0].context.tier, "essential");
        assert_eq!(snapshot.agents[0].recent_files, vec!["src/main.rs"]);
        assert_eq!(snapshot.agents[0].recent_tools, vec!["Write"]);
        assert_eq!(snapshot.agents[0].warnings, vec!["live warning"]);
        assert_eq!(
            snapshot.agents[0].agent_status,
            impulse_ops::AgentStatus::Blocked {
                reason: "merge conflict in src/main.rs".to_string(),
            }
        );
        assert_eq!(
            snapshot.agents[0].role,
            Some(impulse_ops::AgentRole::Worker { parent_pane_id: 42 })
        );
        assert_eq!(snapshot.agents[0].group.as_deref(), Some("review-wave-2"));
        assert_eq!(snapshot.agents[0].tool_invocations.len(), 1);
        assert_eq!(snapshot.agents[0].tool_invocations[0].kind, "write");
        assert_eq!(
            snapshot.agents[0].diff_summary,
            Some(impulse_ops::DiffSummary {
                files_changed: 2,
                lines_added: 18,
                lines_removed: 4,
            })
        );
        assert_eq!(
            snapshot.agents[0].target,
            Some(impulse_ops::MachineTarget::Local {
                workdir: temp.path().display().to_string(),
            })
        );
    }

    fn structured_manager_agent() -> AgentRuntime {
        let role_assignment = impulse_ops::role_assignment::AgentRoleAssignment {
            role: impulse_ops::role_assignment::AgentRoleId::try_new("builder").unwrap(),
            requirements: Vec::new(),
        };
        AgentRuntime {
            agent_status: impulse_ops::AgentStatus::Working {
                task: "durable task".to_string(),
            },
            role: Some(impulse_ops::AgentRole::Coordinator),
            role_assignment: Some(role_assignment.clone()),
            role_compatibility: Some(impulse_ops::role_assignment::RoleCompatibility {
                platform: impulse_ops::agent_registry::AgentPlatformId::try_new("codex").unwrap(),
                role: role_assignment.role,
                checks: Vec::new(),
            }),
            group: Some("durable-wave".to_string()),
            tool_invocations: vec![impulse_ops::ToolInvocationRecord {
                kind: "read".to_string(),
                target: "durable.rs".to_string(),
                timestamp: None,
            }],
            diff_summary: Some(impulse_ops::DiffSummary {
                files_changed: 1,
                lines_added: 1,
                lines_removed: 0,
            }),
            target: Some(impulse_ops::MachineTarget::Local {
                workdir: "/durable".to_string(),
            }),
            ..Default::default()
        }
    }

    fn legacy_agent_runtime(status: &str) -> AgentRuntime {
        let mut legacy_value = serde_json::to_value(AgentRuntime {
            status: status.to_string(),
            active: true,
            ..Default::default()
        })
        .unwrap();
        let legacy_object = legacy_value.as_object_mut().unwrap();
        for field in [
            "current_task",
            "agent_status",
            "role",
            "group",
            "tool_invocations",
            "diff_summary",
            "target",
        ] {
            assert!(legacy_object.remove(field).is_some());
        }
        legacy_object.remove("role_assignment");
        legacy_object.remove("role_compatibility");
        serde_json::from_value(legacy_value).unwrap()
    }

    #[test]
    fn test_overlay_agent_runtime_explicit_idle_preserves_omitted_manager_metadata() {
        let mut target = structured_manager_agent();
        let overlay = AgentRuntime {
            status: "idle".to_string(),
            agent_status: impulse_ops::AgentStatus::Idle,
            ..Default::default()
        };

        overlay_agent_runtime(&mut target, &overlay);

        assert_eq!(target.agent_status, impulse_ops::AgentStatus::Idle);
        assert_eq!(target.role, Some(impulse_ops::AgentRole::Coordinator));
        assert_eq!(
            target
                .role_assignment
                .as_ref()
                .map(|assignment| assignment.role.as_str()),
            Some("builder")
        );
        assert!(target.role_compatibility.is_some());
        assert_eq!(target.group.as_deref(), Some("durable-wave"));
        assert_eq!(target.tool_invocations.len(), 1);
        assert!(target.diff_summary.is_some());
        assert!(target.target.is_some());
    }

    #[test]
    fn test_overlay_agent_runtime_preserves_newer_typed_role_facts_from_legacy_overlay() {
        let mut target = structured_manager_agent();
        let overlay = legacy_agent_runtime("working: legacy publisher");

        overlay_agent_runtime(&mut target, &overlay);

        assert_eq!(
            target
                .role_assignment
                .as_ref()
                .map(|assignment| assignment.role.as_str()),
            Some("builder")
        );
        assert_eq!(
            target
                .role_compatibility
                .as_ref()
                .map(|compatibility| compatibility.platform.as_str()),
            Some("codex")
        );
    }

    #[test]
    fn test_overlay_agent_runtime_applies_populated_typed_role_facts() {
        let assignment = impulse_ops::role_assignment::AgentRoleAssignment {
            role: impulse_ops::role_assignment::AgentRoleId::try_new("reviewer").unwrap(),
            requirements: Vec::new(),
        };
        let compatibility = impulse_ops::role_assignment::RoleCompatibility {
            platform: impulse_ops::agent_registry::AgentPlatformId::try_new("claude-code").unwrap(),
            role: assignment.role.clone(),
            checks: Vec::new(),
        };
        let mut target = AgentRuntime::default();
        let overlay = AgentRuntime {
            role_assignment: Some(assignment.clone()),
            role_compatibility: Some(compatibility.clone()),
            ..Default::default()
        };

        overlay_agent_runtime(&mut target, &overlay);

        assert_eq!(target.role_assignment, Some(assignment));
        assert_eq!(target.role_compatibility, Some(compatibility));
    }

    #[test]
    fn test_overlay_agent_runtime_working_status_preserves_task_exactly() {
        let mut target = AgentRuntime::default();
        let overlay = AgentRuntime {
            status: "working: reconcile daemon state".to_string(),
            agent_status: impulse_ops::AgentStatus::Working {
                task: "reconcile daemon state".to_string(),
            },
            ..Default::default()
        };

        overlay_agent_runtime(&mut target, &overlay);

        assert_eq!(target.status, "working: reconcile daemon state");
        assert_eq!(
            target.agent_status,
            impulse_ops::AgentStatus::Working {
                task: "reconcile daemon state".to_string(),
            }
        );
    }

    #[test]
    fn test_overlay_agent_runtime_recognizes_legacy_lifecycle_states() {
        let cases = [
            ("starting", impulse_ops::AgentStatus::Starting),
            ("idle", impulse_ops::AgentStatus::Idle),
            (
                "working: compile workspace",
                impulse_ops::AgentStatus::Working {
                    task: "compile workspace".to_string(),
                },
            ),
            (
                "working:",
                impulse_ops::AgentStatus::Working {
                    task: String::new(),
                },
            ),
            (
                "blocked: waiting for review",
                impulse_ops::AgentStatus::Blocked {
                    reason: "waiting for review".to_string(),
                },
            ),
            (
                "blocked:",
                impulse_ops::AgentStatus::Blocked {
                    reason: String::new(),
                },
            ),
            ("interrupted", impulse_ops::AgentStatus::Interrupted),
            ("completed", impulse_ops::AgentStatus::Completed),
        ];

        for (legacy_status, expected) in cases {
            let mut target = structured_manager_agent();
            let overlay = legacy_agent_runtime(legacy_status);

            overlay_agent_runtime(&mut target, &overlay);

            assert_eq!(target.agent_status, expected, "legacy {legacy_status}");
        }
    }

    #[test]
    fn test_overlay_agent_runtime_legacy_json_preserves_structured_manager_state() {
        let mut target = structured_manager_agent();
        let overlay = legacy_agent_runtime("active");

        overlay_agent_runtime(&mut target, &overlay);

        assert_eq!(
            target.agent_status,
            impulse_ops::AgentStatus::Working {
                task: "durable task".to_string(),
            }
        );
        assert_eq!(target.role, Some(impulse_ops::AgentRole::Coordinator));
        assert_eq!(target.group.as_deref(), Some("durable-wave"));
        assert_eq!(target.tool_invocations.len(), 1);
        assert!(target.diff_summary.is_some());
        assert!(target.target.is_some());
    }

    #[test]
    fn test_overlay_agent_runtime_unions_and_deduplicates_warnings() {
        let mut target = AgentRuntime {
            warnings: vec!["durable".to_string(), "shared".to_string()],
            ..Default::default()
        };
        let overlay = AgentRuntime {
            warnings: vec!["live".to_string(), "shared".to_string()],
            ..Default::default()
        };

        overlay_agent_runtime(&mut target, &overlay);

        assert_eq!(target.warnings, vec!["durable", "shared", "live"]);
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

    #[test]
    fn test_terminal_ops_store_freshness_uses_daemon_receipt_time_for_old_publisher_clock() {
        let now = Utc::now();
        let published_at = (now - ChronoDuration::days(1)).to_rfc3339();
        let mut store = TerminalOpsTelemetryStore::default();
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "clock-behind".to_string(),
                published_at: published_at.clone(),
                ..Default::default()
            },
        );

        let reports = store.fresh_reports("demo", now);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].source_id, "clock-behind");
        assert_eq!(reports[0].published_at, published_at);
    }

    #[test]
    fn test_terminal_ops_store_preserves_omitted_same_source_role_facts_and_accepts_newer_values() {
        let mut store = TerminalOpsTelemetryStore::default();
        let mut enriched = structured_manager_agent();
        enriched.id = "agent-1".to_string();
        enriched.session_id = Some("session-a".to_string());
        enriched.current_task = Some("durable task".to_string());
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "desktop-runtime".to_string(),
                published_at: "2026-07-13T00:00:00Z".to_string(),
                agents: vec![enriched],
                ..Default::default()
            },
        );

        let mut legacy = legacy_agent_runtime("working: legacy publisher");
        legacy.id = "agent-1".to_string();
        legacy.session_id = Some("session-a".to_string());
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "desktop-runtime".to_string(),
                published_at: "2026-07-13T00:00:01Z".to_string(),
                agents: vec![legacy],
                ..Default::default()
            },
        );

        let after_legacy = store.fresh_reports("demo", Utc::now());
        assert_eq!(after_legacy.len(), 1);
        assert_eq!(after_legacy[0].published_at, "2026-07-13T00:00:01Z");
        assert_eq!(
            after_legacy[0].agents[0]
                .role_assignment
                .as_ref()
                .map(|assignment| assignment.role.as_str()),
            Some("builder")
        );
        assert!(after_legacy[0].agents[0].role_compatibility.is_some());
        assert_eq!(
            after_legacy[0].agents[0].current_task.as_deref(),
            Some("durable task")
        );

        let assignment = impulse_ops::role_assignment::AgentRoleAssignment {
            role: impulse_ops::role_assignment::AgentRoleId::try_new("reviewer").unwrap(),
            requirements: Vec::new(),
        };
        let compatibility = impulse_ops::role_assignment::RoleCompatibility {
            platform: impulse_ops::agent_registry::AgentPlatformId::try_new("claude-code").unwrap(),
            role: assignment.role.clone(),
            checks: Vec::new(),
        };
        let latest = AgentRuntime {
            id: "agent-1".to_string(),
            session_id: Some("session-a".to_string()),
            current_task: Some("review latest diff".to_string()),
            role_assignment: Some(assignment),
            role_compatibility: Some(compatibility),
            ..Default::default()
        };
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "desktop-runtime".to_string(),
                published_at: "2026-07-13T00:00:02Z".to_string(),
                agents: vec![latest],
                ..Default::default()
            },
        );

        let after_enriched = store.fresh_reports("demo", Utc::now());
        assert_eq!(
            after_enriched[0].agents[0]
                .role_assignment
                .as_ref()
                .map(|assignment| assignment.role.as_str()),
            Some("reviewer")
        );
        assert_eq!(
            after_enriched[0].agents[0]
                .role_compatibility
                .as_ref()
                .map(|compatibility| compatibility.platform.as_str()),
            Some("claude-code")
        );
        assert_eq!(
            after_enriched[0].agents[0].current_task.as_deref(),
            Some("review latest diff")
        );
    }

    #[test]
    fn test_terminal_ops_store_does_not_inherit_role_facts_across_agent_sessions() {
        let mut store = TerminalOpsTelemetryStore::default();
        let mut enriched = structured_manager_agent();
        enriched.id = "agent-1".to_string();
        enriched.session_id = Some("session-a".to_string());
        enriched.current_task = Some("session a task".to_string());
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "desktop-runtime".to_string(),
                agents: vec![enriched],
                ..Default::default()
            },
        );

        let mut next_session = legacy_agent_runtime("starting");
        next_session.id = "agent-1".to_string();
        next_session.session_id = Some("session-b".to_string());
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "desktop-runtime".to_string(),
                agents: vec![next_session],
                ..Default::default()
            },
        );

        let reports = store.fresh_reports("demo", Utc::now());
        let agent = &reports[0].agents[0];
        assert_eq!(agent.session_id.as_deref(), Some("session-b"));
        assert!(agent.current_task.is_none());
        assert!(agent.role_assignment.is_none());
        assert!(agent.role_compatibility.is_none());
    }

    #[test]
    fn test_terminal_ops_store_empty_same_source_report_removes_previous_agents() {
        let mut store = TerminalOpsTelemetryStore::default();
        let mut enriched = structured_manager_agent();
        enriched.id = "agent-1".to_string();
        enriched.session_id = Some("session-a".to_string());
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "desktop-runtime".to_string(),
                agents: vec![enriched],
                ..Default::default()
            },
        );
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "desktop-runtime".to_string(),
                agents: Vec::new(),
                ..Default::default()
            },
        );

        let reports = store.fresh_reports("demo", Utc::now());
        assert_eq!(reports.len(), 1);
        assert!(reports[0].agents.is_empty());
    }

    #[test]
    fn test_terminal_ops_store_freshness_uses_daemon_receipt_time_for_future_publisher_clock() {
        let now = Utc::now();
        let mut store = TerminalOpsTelemetryStore::default();
        store.publish(
            "demo",
            TerminalOpsReport {
                source_id: "clock-ahead".to_string(),
                published_at: (now + ChronoDuration::days(1)).to_rfc3339(),
                ..Default::default()
            },
        );

        let reports = store.fresh_reports(
            "demo",
            now + ChronoDuration::seconds(TELEMETRY_PURGE_AFTER_SECS + 1),
        );

        assert!(reports.is_empty());
    }
}
