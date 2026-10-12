//! Context lifecycle methods for TerminalsView.
//!
//! Extracted from terminals.rs — context ticks, signal collection,
//! threshold injections, and workbench data accessors.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use impulse_term::context::{ContextTier, InsightType};

use crate::state::PollerCommand;
use crate::widgets::signal_bus::{GuiSignal, SignalKind, SignalUrgency};

use super::terminals::TerminalsView;

impl TerminalsView {
    /// Process pending init injections (called from app.rs update loop).
    pub fn process_pending_injections(&mut self, impulse_home: &Path) {
        let now = Instant::now();

        // Drain ready injections (stable alternative to nightly drain_filter).
        let mut i = 0;
        while i < self.pending_injections.len() {
            if now >= self.pending_injections[i].inject_at {
                let pending = self.pending_injections.remove(i);

                if let Some(tab) = self.tabs.get_mut(&pending.tab_id) {
                    let identity = crate::identity::load_identity(impulse_home).unwrap_or_default();

                    let project_name = pending
                        .target_dir
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let context = super::terminals::build_init_context(
                        &identity,
                        &pending.target_dir,
                        tab.agent_name,
                    );

                    match tab.panel.context_bridge().inject_context(&context) {
                        Ok(()) => log::info!(
                            "Injected init context into tab {} ({})",
                            pending.tab_id,
                            project_name
                        ),
                        Err(e) => log::warn!(
                            "Failed to inject init context into tab {}: {}",
                            pending.tab_id,
                            e
                        ),
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    /// Build workbench agent runtime data for all tabs.
    pub fn workbench_agents(&self) -> Vec<impulse_ops::AgentRuntime> {
        self.tabs
            .iter()
            .map(|(id, tab)| {
                let health = tab.panel.context_health();
                let recent_insights = tab
                    .panel
                    .insights()
                    .iter()
                    .rev()
                    .take(5)
                    .map(|insight| impulse_ops::InsightRecord {
                        timestamp: Some(insight.timestamp.to_rfc3339()),
                        agent_label: tab.agent_name.to_string(),
                        kind: insight.insight_type.as_str().to_string(),
                        content: insight.content.clone(),
                    })
                    .collect::<Vec<_>>();

                let mut warnings = Vec::new();
                if matches!(health.tier, ContextTier::Critical | ContextTier::Minimal) {
                    warnings.push(format!(
                        "Context tier is {} and needs review soon",
                        health.tier.as_str()
                    ));
                }
                if !tab.panel.is_alive() {
                    warnings.push("Terminal process is no longer alive".to_string());
                }

                impulse_ops::AgentRuntime {
                    id: format!("tab-{}", id),
                    label: tab.label.clone(),
                    backend_kind: tab.agent_name.to_string(),
                    session_id: None,
                    ephemeral: true,
                    working_directory: tab.target_dir.display().to_string(),
                    status: if tab.panel.is_alive() {
                        "active".to_string()
                    } else {
                        "stopped".to_string()
                    },
                    current_task: recent_insights
                        .first()
                        .map(|insight| insight.content.clone()),
                    active: tab.panel.is_alive(),
                    context: impulse_ops::ContextHealthSummary {
                        tier: health.tier.as_str().to_string(),
                        usage_fraction: health.usage_fraction,
                        estimated_tokens: health.estimated_tokens,
                        window_tokens: health.window_tokens,
                        compaction_count: health.compaction_count,
                        injection_count: health.injection_count,
                        pending_review_count: self.pending_injections.len(),
                        recent_insights,
                    },
                    recent_files: tab
                        .panel
                        .insights()
                        .iter()
                        .filter(|insight| insight.insight_type == InsightType::FileModified)
                        .map(|insight| insight.content.clone())
                        .collect(),
                    recent_tools: Vec::new(),
                    warnings,
                    agent_status: impulse_ops::AgentStatus::default(),
                    role: None,
                    role_assignment: None,
                    role_compatibility: None,
                    group: None,
                    tool_invocations: Vec::new(),
                    diff_summary: None,
                    target: None,
                }
            })
            .collect()
    }

    /// Aggregate context health summary across all tabs.
    pub fn workbench_context(&self) -> impulse_ops::ContextHealthSummary {
        let mut summary = impulse_ops::ContextHealthSummary {
            tier: "steady".to_string(),
            pending_review_count: self.pending_injections.len(),
            ..Default::default()
        };

        let mut recent_insights = Vec::new();
        for tab in self.tabs.values() {
            let health = tab.panel.context_health();
            if health.usage_fraction > summary.usage_fraction {
                summary.usage_fraction = health.usage_fraction;
                summary.tier = health.tier.as_str().to_string();
                summary.estimated_tokens = health.estimated_tokens;
                summary.window_tokens = health.window_tokens;
            }
            summary.compaction_count += health.compaction_count;
            summary.injection_count += health.injection_count;
            recent_insights.extend(tab.panel.insights().iter().rev().take(4).map(|insight| {
                impulse_ops::InsightRecord {
                    timestamp: Some(insight.timestamp.to_rfc3339()),
                    agent_label: tab.agent_name.to_string(),
                    kind: insight.insight_type.as_str().to_string(),
                    content: insight.content.clone(),
                }
            }));
        }
        recent_insights.truncate(20);
        summary.recent_insights = recent_insights;
        summary
    }

    /// Build intervention recommendations from agent states.
    pub fn workbench_interventions(&self) -> Vec<impulse_ops::InterventionRecommendation> {
        let mut interventions = Vec::new();
        for agent in self.workbench_agents() {
            if matches!(agent.context.tier.as_str(), "critical" | "minimal") {
                interventions.push(impulse_ops::InterventionRecommendation {
                    id: format!("review-{}", agent.id),
                    title: format!("Review {}", agent.label),
                    description: format!(
                        "{} is at context tier {} ({} tokens of {}).",
                        agent.label,
                        agent.context.tier,
                        agent.context.estimated_tokens,
                        agent.context.window_tokens
                    ),
                    severity: if agent.context.tier == "minimal" {
                        "urgent".to_string()
                    } else {
                        "warning".to_string()
                    },
                    action_kind: "focus_agent".to_string(),
                    action_label: "Focus Agent".to_string(),
                    target_agent_id: Some(agent.id.clone()),
                });
            }
        }
        interventions
    }

    /// Run context extraction tick on all alive panels.
    ///
    /// Collects newly extracted insights and persists them to LIVE_INSIGHTS.jsonl.
    /// Also forwards FileModified insights to the daemon for session tracking.
    pub fn context_tick(&mut self) {
        let mut new_insights: Vec<impulse_term::context::ExtractedInsight> = Vec::new();
        let mut file_tracks: Vec<(String, String)> = Vec::new(); // (session_id, file_path)

        for tab in self.tabs.values_mut() {
            if tab.panel.is_alive() {
                let extracted = tab.panel.context_bridge().extract_tick();
                if !extracted.is_empty() {
                    // Track new file modifications with daemon.
                    if let Some(ref session_id) = tab.daemon_session_id {
                        for insight in &extracted {
                            if insight.insight_type == InsightType::FileModified {
                                file_tracks.push((session_id.clone(), insight.content.clone()));
                            }
                        }
                    }
                    new_insights.extend(extracted);
                }
            }
        }

        if !new_insights.is_empty() {
            self.persist_insights(&new_insights);
        }

        // Forward file tracks to daemon (deduplicating per session).
        if let Some(ref cmd_tx) = self.poller_cmd {
            for (session_id, file_path) in file_tracks {
                let already_tracked = self
                    .tracked_files
                    .entry(session_id.clone())
                    .or_default()
                    .contains(&file_path);
                if !already_tracked {
                    self.tracked_files
                        .entry(session_id.clone())
                        .or_default()
                        .insert(file_path.clone());
                    let _ = cmd_tx.send(PollerCommand::TrackFile {
                        session_id,
                        file_path,
                    });
                }
            }
        }
    }

    /// Compare tab states against snapshots and emit signals for changes.
    ///
    /// Called after `context_tick()` in the 3-second tick block. Detects:
    /// - New errors/task completions from insight diffs
    /// - Compaction events from compaction count changes
    /// - Context tier crossings (60%, 80%)
    /// - Cross-tab file conflicts from modified_files set intersection
    pub fn collect_signals(&mut self) -> Vec<GuiSignal> {
        if self.tabs.is_empty() {
            return Vec::new();
        }
        let mut signals = Vec::new();
        let now = Instant::now();
        let tab_ids: Vec<u64> = self.tabs.keys().copied().collect();

        // Phase 1: Collect per-tab signals by comparing against snapshots.
        for &id in &tab_ids {
            let Some(tab) = self.tabs.get(&id) else {
                continue;
            };
            if !tab.panel.is_alive() {
                continue;
            }

            let health = tab.panel.context_health();
            let insights = tab.panel.insights();
            let snapshot = self.tab_snapshots.entry(id).or_default();

            // Check for new insights since last snapshot.
            if insights.len() > snapshot.insight_count {
                let new_insights = &insights[snapshot.insight_count..];
                for insight in new_insights {
                    match insight.insight_type {
                        InsightType::ErrorEncountered => {
                            signals.push(GuiSignal {
                                kind: SignalKind::ErrorEncountered,
                                urgency: SignalUrgency::Important,
                                tab_id: Some(id),
                                message: format!(
                                    "[{}] Error: {}",
                                    tab.label,
                                    impulse_term::context::truncate_insight(&insight.content, 80)
                                ),
                                created_at: now,
                            });
                        }
                        InsightType::TaskCompleted => {
                            signals.push(GuiSignal {
                                kind: SignalKind::TaskCompleted,
                                urgency: SignalUrgency::Important,
                                tab_id: Some(id),
                                message: format!(
                                    "[{}] Task completed: {}",
                                    tab.label,
                                    impulse_term::context::truncate_insight(&insight.content, 60)
                                ),
                                created_at: now,
                            });
                        }
                        InsightType::FileModified => {
                            snapshot.modified_files.insert(insight.content.clone());
                        }
                        InsightType::DecisionMade => {}
                    }
                }
                snapshot.insight_count = insights.len();
            }

            // Check compaction count changes.
            if health.compaction_count > snapshot.compaction_count {
                signals.push(GuiSignal {
                    kind: SignalKind::CompactionDetected,
                    urgency: SignalUrgency::Important,
                    tab_id: Some(id),
                    message: format!(
                        "[{}] Context compacted \u{2014} some memory was lost",
                        tab.label
                    ),
                    created_at: now,
                });
                snapshot.compaction_count = health.compaction_count;
            }

            // Check tier crossings.
            let current_tier = health.tier;
            let previous_tier = snapshot.tier;
            if previous_tier != Some(current_tier) {
                match current_tier {
                    ContextTier::Critical => {
                        signals.push(GuiSignal {
                            kind: SignalKind::ContextThreshold { pct: 60 },
                            urgency: SignalUrgency::Important,
                            tab_id: Some(id),
                            message: format!(
                                "[{}] Context at 60% \u{2014} consider compacting soon",
                                tab.label
                            ),
                            created_at: now,
                        });
                    }
                    ContextTier::Minimal => {
                        signals.push(GuiSignal {
                            kind: SignalKind::ContextThreshold { pct: 80 },
                            urgency: SignalUrgency::Urgent,
                            tab_id: Some(id),
                            message: format!(
                                "[{}] Context at 80% \u{2014} compact or start fresh",
                                tab.label
                            ),
                            created_at: now,
                        });
                    }
                    _ => {}
                }
                snapshot.tier = Some(current_tier);
            }
        }

        // Phase 2: Single-pass conflict detection via file→owners map.
        let mut file_owners: HashMap<&str, Vec<(u64, &str)>> = HashMap::new();
        for &id in &tab_ids {
            if let Some(snap) = self.tab_snapshots.get(&id) {
                let label = self.tabs.get(&id).map(|t| t.label.as_str()).unwrap_or("");
                for file in &snap.modified_files {
                    file_owners
                        .entry(file.as_str())
                        .or_default()
                        .push((id, label));
                }
            }
        }

        self.active_conflicts.clear();
        for (file, owners) in &file_owners {
            if owners.len() < 2 {
                continue;
            }
            let mut push_conflict = |tab_id: u64, other_label: &str| {
                signals.push(GuiSignal {
                    kind: SignalKind::FileConflict {
                        path: file.to_string(),
                        other_tab: other_label.to_string(),
                    },
                    urgency: SignalUrgency::Urgent,
                    tab_id: Some(tab_id),
                    message: format!(
                        "Conflict: {} edited in both tabs (also in {})",
                        file, other_label
                    ),
                    created_at: now,
                });
            };
            for i in 0..owners.len() {
                for j in (i + 1)..owners.len() {
                    let (id_a, label_a) = owners[i];
                    let (id_b, label_b) = owners[j];
                    // Emit one signal per direction (A sees B, B sees A).
                    push_conflict(id_a, label_b);
                    self.active_conflicts
                        .entry(id_a)
                        .or_default()
                        .push((file.to_string(), label_b.to_string()));

                    push_conflict(id_b, label_a);
                    self.active_conflicts
                        .entry(id_b)
                        .or_default()
                        .push((file.to_string(), label_a.to_string()));
                }
            }
        }

        signals
    }

    /// Check tier crossings and inject refresh context on threshold changes.
    ///
    /// Tracks `last_injected_tier` per tab. When a tier crossing is detected,
    /// builds refresh context with tier info, cross-pane insights, and recent
    /// GENOME decisions, then injects via the ContextBridge.
    pub fn check_threshold_injections(
        &mut self,
        genome_decisions: &[String],
        active_sessions: &[String],
        recent_history: &[String],
    ) {
        // Phase 1: Collect info immutably — which tabs need injection and what context.
        let mut injections: Vec<(u64, String)> = Vec::new();

        let tab_ids: Vec<u64> = self.tabs.keys().copied().collect();

        for &id in &tab_ids {
            let Some(tab) = self.tabs.get(&id) else {
                continue;
            };
            if !tab.panel.is_alive() {
                continue;
            }
            let current_tier = tab.panel.current_tier();

            // Only inject on meaningful tiers (not None, not PostCompaction).
            let should_inject = matches!(
                current_tier,
                ContextTier::Essential | ContextTier::Critical | ContextTier::Minimal
            );
            if !should_inject {
                continue;
            }

            // Check if this is a new tier crossing.
            let last_tier = self.last_injected_tiers.get(&id).copied();
            if last_tier == Some(current_tier) {
                continue;
            }
            self.last_injected_tiers.insert(id, current_tier);

            // Collect cross-pane insights from other alive panes (immutable access).
            let mut cross_pane = Vec::new();
            for (&other_id, other_tab) in &self.tabs {
                if other_id == id || !other_tab.panel.is_alive() {
                    continue;
                }
                for insight in other_tab.panel.insights().iter().rev().take(3) {
                    cross_pane.push(format!(
                        "  - [{}] {}: {}",
                        other_tab.label,
                        insight.insight_type.as_str(),
                        insight.content
                    ));
                }
            }

            // Build refresh context via extracted pure function.
            if let Some(refresh) = super::memory_persistence::build_refresh_context(
                current_tier,
                &cross_pane,
                genome_decisions,
                active_sessions,
                recent_history,
            ) {
                injections.push((id, refresh));
            }
        }

        // Phase 2: Inject via ContextBridge (requires &mut).
        for (id, refresh) in injections {
            if let Some(tab) = self.tabs.get_mut(&id) {
                match tab.panel.context_bridge().inject_context(&refresh) {
                    Ok(()) => {
                        log::info!("Injected refresh context into tab {}", id);
                    }
                    Err(e) => {
                        log::warn!("Threshold injection failed for tab {}: {}", id, e);
                    }
                }
            }
        }
    }
}
