use super::*;

/// Handle conflict resolution from the TUI.
/// Updates the mier_recommendations to mark a conflict as resolved.
pub(crate) fn handle_conflict_resolution(
    state: &mut TuiState,
    resolution: crate::agent::coordinator::ConflictResolution,
) {
    // Get active file conflicts from recommendations
    let conflict_recommendations: Vec<_> = state
        .mier_recommendations
        .iter()
        .filter(|r| {
            matches!(
                r.recommendation_type,
                crate::agent::coordinator::RecommendationType::FileConflict
            )
        })
        .collect();

    if conflict_recommendations.is_empty() {
        state.status_message = Some("No active conflicts to resolve".to_string());
        return;
    }

    // Resolve the selected conflict (cycle through if multiple)
    let idx = state.selected_conflict_index % conflict_recommendations.len();
    if let Some(rec) = conflict_recommendations.get(idx) {
        let file_path = rec
            .description
            .strip_prefix("Multiple agents modifying: ")
            .unwrap_or(&rec.description)
            .to_string();

        // Update recommendation to show resolution
        for r in state.mier_recommendations.iter_mut() {
            if r.recommendation_type == crate::agent::coordinator::RecommendationType::FileConflict
                && r.description.contains(&file_path)
            {
                r.action = format!("Resolved via {}", resolution.as_str());
                r.description = format!("{} (RESOLVED)", file_path);
            }
        }

        state.status_message = Some(format!(
            "Resolved conflict: {} ({})",
            file_path,
            resolution.as_str()
        ));

        // Emit resolution notification
        let bus = state.notification_bus.clone();
        let path = file_path.clone();
        let res_str = resolution.as_str().to_string();
        tokio::spawn(async move {
            crate::notification::emit_conflict_resolved(&bus, &path, &res_str).await;
        });
    }
}

/// Process context lifecycle events: pending injections, threshold monitoring,
/// compaction detection, and output extraction.
/// Called from the event loop every 5 seconds.
pub(crate) fn context_lifecycle_tick(state: &mut TuiState) {
    if !state.context_lifecycle_enabled {
        return;
    }

    let pm = match state.pane_manager.as_ref() {
        Some(pm) => pm,
        None => return,
    };

    // 1. Process pending injections (initial context after spawn delay)
    let mut completed_injections = Vec::new();
    for (idx, pending) in state.pending_injections.iter().enumerate() {
        let elapsed_ms = pending.scheduled_at.elapsed().as_millis() as u64;
        let delay = pending.agent_kind.startup_delay_ms();

        if elapsed_ms < delay {
            continue;
        }

        // Find the pane and check it has produced output
        if let Some(pane) = pm.find_by_id(pending.pane_id) {
            if !pane.is_alive() || pane.output_bytes() == 0 {
                continue;
            }

            // Gather cross-pane insights from other panes
            let cross_insights: Vec<_> = state
                .context_monitor
                .pane_states
                .values()
                .filter(|s| s.pane_id != pending.pane_id)
                .flat_map(|s| s.extracted_insights.iter().cloned())
                .take(crate::context_lifecycle::types::MAX_CROSS_PANE_INSIGHTS)
                .collect();

            let msg = ContextInjector::build_init_message(
                pending.agent_kind,
                None,
                &pending.pane_name,
                &cross_insights,
            );

            if pane.write_input(msg.as_bytes()).is_ok() {
                let _ = pane.write_input(b"\n");
                // Mark injection done in monitor state
                if let Some(pane_state) =
                    state.context_monitor.pane_states.get_mut(&pending.pane_id)
                {
                    pane_state.initial_injection_done = true;
                    pane_state.mark_injected();
                }
                // Emit notification
                let bus = state.notification_bus.clone();
                let pname = pending.pane_name.clone();
                let msg_len = msg.len();
                tokio::runtime::Handle::current().block_on(bus.publish(
                    crate::notification::NotificationEvent::ContextRefreshed {
                        pane_id: pending.pane_id,
                        pane_name: pname.clone(),
                        tier: "init".to_string(),
                        size_chars: msg_len,
                    },
                ));
                state.mier_activity_feed.push(MierFeedEntry {
                    timestamp: std::time::Instant::now(),
                    kind: MierFeedKind::Injection,
                    message: format!("Init injection → {} ({} chars)", pname, msg_len),
                });
            }
            completed_injections.push(idx);
        }
    }
    // Remove completed injections (reverse order to preserve indices)
    for idx in completed_injections.into_iter().rev() {
        state.pending_injections.remove(idx);
    }

    // 2. For each alive pane: monitor thresholds, detect compaction, extract insights
    let window_tokens = state.context_monitor.window_tokens;
    let alive_ids: Vec<usize> = pm
        .panes
        .iter()
        .filter(|p| p.is_alive())
        .map(|p| p.id)
        .collect();

    // Collect pane data with scrollback scan (up to 200 lines back)
    let pane_data: Vec<(usize, u64, String)> = pm
        .panes
        .iter()
        .filter(|p| p.is_alive())
        .map(|p| {
            let current = p.screen_snapshot().contents();
            let scrollback = p.scrollback_len();
            let combined = if scrollback > 0 {
                let mut pages = Vec::new();
                let mut offset = 24;
                while offset <= scrollback.min(200) {
                    pages.push(p.screen_snapshot_at_offset(offset).contents());
                    offset += 24;
                }
                if pages.is_empty() {
                    current
                } else {
                    // Scrollback is older content, so prepend it
                    pages.reverse();
                    pages.push(current);
                    pages.join("\n")
                }
            } else {
                current
            };
            (p.id, p.output_bytes(), combined)
        })
        .collect();

    let mut refresh_actions = Vec::new();

    for (pane_id, output_bytes, screen_text) in &pane_data {
        // Token threshold monitoring
        if let Some(action) = state.context_monitor.check_pane(*pane_id, *output_bytes) {
            refresh_actions.push(action);
        }

        // Compaction detection
        if let Some(pane_state) = state.context_monitor.pane_states.get_mut(pane_id) {
            if let Some(action) =
                CompactionDetector::check_pane(pane_state, screen_text, window_tokens)
            {
                refresh_actions.push(action);
            }

            // Output extraction (every 30s per pane)
            OutputExtractor::check_pane(pane_state, screen_text);
        }
    }

    // 2b. Refine phase: cross-pane coordination via ImpulseAgent
    {
        let all_insights: Vec<_> = state
            .context_monitor
            .pane_states
            .values()
            .flat_map(|s| s.extracted_insights.iter().cloned())
            .collect();

        if !all_insights.is_empty() {
            // Feed insights to intent detection
            for insight in &all_insights {
                let agent_type = match insight.agent_kind {
                    AgentKind::ClaudeCode => crate::context_lifecycle::AgentType::Claude,
                    AgentKind::Codex => crate::context_lifecycle::AgentType::Codex,
                    AgentKind::OpenCode => crate::context_lifecycle::AgentType::OpenCode,
                    AgentKind::GenericShell => crate::context_lifecycle::AgentType::Shell,
                };
                let activity_type = match insight.insight_type {
                    crate::context_lifecycle::types::InsightType::FileModified => {
                        crate::context_lifecycle::ActivityType::FileEdit
                    }
                    crate::context_lifecycle::types::InsightType::ErrorEncountered => {
                        crate::context_lifecycle::ActivityType::Error
                    }
                    crate::context_lifecycle::types::InsightType::TaskCompleted
                    | crate::context_lifecycle::types::InsightType::DecisionMade
                    | crate::context_lifecycle::types::InsightType::ToolInvocation
                    | crate::context_lifecycle::types::InsightType::DiffDetected
                    | crate::context_lifecycle::types::InsightType::DelegationDetected
                    | crate::context_lifecycle::types::InsightType::RemoteConnection => {
                        crate::context_lifecycle::ActivityType::Output
                    }
                };
                let activity = crate::context_lifecycle::Activity::new(
                    format!("pane-{}", insight.pane_id),
                    agent_type,
                    activity_type,
                )
                .with_target(insight.content.clone())
                .with_details(vec![insight.insight_type.as_str().to_string()]);
                state.intent_store.detect(activity);
            }

            // Run full coordination (file conflicts, cross-pane errors, pane summaries)
            if let Some(ref mut agent) = state.impulse_agent {
                let coordination = agent.coordinate_full(&all_insights);
                let new_recs = coordination.recommendations;
                let notification_bus = state.notification_bus.clone();
                let state_clone = state.state.clone();
                for rec in &new_recs {
                    // Track conflict detection for notification banner and emit notification
                    if matches!(
                        rec.recommendation_type,
                        crate::agent::coordinator::RecommendationType::FileConflict
                    ) {
                        state.last_conflict_notification = Some(std::time::Instant::now());
                        // Emit conflict notification
                        let file_path = rec.description.clone();
                        let panes = rec.panes_involved.clone();
                        let bus_clone = notification_bus.clone();
                        let state_for_webhook = state_clone.clone();
                        tokio::spawn(async move {
                            crate::notification::emit_conflict_detected(
                                &bus_clone,
                                &file_path,
                                panes.clone(),
                                "Multiple agents modifying same file",
                            )
                            .await;

                            // Send webhook notification if configured
                            if let Ok(config) = state_for_webhook.config_snapshot() {
                                if config.conflict_webhook_enabled {
                                    if let Some(ref webhook_url) = config.conflict_webhook_url {
                                        crate::notification::send_conflict_webhook(
                                            Some(webhook_url),
                                            &file_path,
                                            panes,
                                            "Multiple agents modifying same file",
                                        )
                                        .await;
                                    }
                                }
                            }
                        });
                    }
                    state.mier_activity_feed.push(MierFeedEntry {
                        timestamp: std::time::Instant::now(),
                        kind: MierFeedKind::Recommendation,
                        message: format!(
                            "[{}] {}",
                            rec.recommendation_type.as_str(),
                            rec.description
                        ),
                    });
                }
                state.mier_recommendations.extend(new_recs);
                if state.mier_recommendations.len() > 20 {
                    let excess = state.mier_recommendations.len() - 20;
                    state.mier_recommendations.drain(..excess);
                }

                // Surface pane summaries in the activity feed
                for (pane_label, summaries) in &coordination.pane_summaries {
                    let summary_count = summaries.len();
                    state.mier_activity_feed.push(MierFeedEntry {
                        timestamp: std::time::Instant::now(),
                        kind: MierFeedKind::PaneSummary,
                        message: format!(
                            "{}: {} insight{}",
                            pane_label,
                            summary_count,
                            if summary_count == 1 { "" } else { "s" }
                        ),
                    });
                }
            }
        }

        // Bound activity feed at 50
        if state.mier_activity_feed.len() > 50 {
            let excess = state.mier_activity_feed.len() - 50;
            state.mier_activity_feed.drain(..excess);
        }
    }

    // 3. Process refresh actions (inject context at appropriate tier)
    let pm = match state.pane_manager.as_ref() {
        Some(pm) => pm,
        None => return,
    };

    for action in refresh_actions {
        match action {
            crate::context_lifecycle::MonitorAction::RefreshContext { pane_id, tier } => {
                if let Some(pane) = pm.find_by_id(pane_id) {
                    let agent_kind = state
                        .context_monitor
                        .pane_states
                        .get(&pane_id)
                        .map(|s| s.agent_kind)
                        .unwrap_or(AgentKind::GenericShell);

                    let cross_insights: Vec<_> = state
                        .context_monitor
                        .pane_states
                        .values()
                        .filter(|s| s.pane_id != pane_id)
                        .flat_map(|s| s.extracted_insights.iter().cloned())
                        .take(crate::context_lifecycle::types::MAX_CROSS_PANE_INSIGHTS)
                        .collect();

                    let pane_name = pane.name.clone();
                    let msg = ContextInjector::build_refresh_message(
                        agent_kind,
                        tier,
                        &pane_name,
                        &cross_insights,
                    );

                    if pane.write_input(msg.as_bytes()).is_ok() {
                        let _ = pane.write_input(b"\n");
                        if let Some(ps) = state.context_monitor.pane_states.get_mut(&pane_id) {
                            ps.mark_injected();
                        }
                        // Emit threshold notification
                        let bus = state.notification_bus.clone();
                        let tier_str = tier.as_str().to_string();
                        let pct = state
                            .context_monitor
                            .pane_states
                            .get(&pane_id)
                            .map(|s| {
                                if window_tokens > 0 {
                                    ((s.estimated_tokens as f64 / window_tokens as f64) * 100.0)
                                        as u8
                                } else {
                                    0
                                }
                            })
                            .unwrap_or(0);
                        tokio::runtime::Handle::current().block_on(bus.publish(
                            crate::notification::NotificationEvent::ContextThresholdCrossed {
                                pane_id,
                                pane_name: pane_name.clone(),
                                threshold_pct: pct,
                                tier: tier_str.clone(),
                            },
                        ));
                        state.mier_activity_feed.push(MierFeedEntry {
                            timestamp: std::time::Instant::now(),
                            kind: MierFeedKind::ThresholdCrossed,
                            message: format!("{}% ({}) → {}", pct, tier_str, pane_name),
                        });
                    }
                }
            }
            crate::context_lifecycle::MonitorAction::CompactionDetected { pane_id } => {
                if let Some(pane) = pm.find_by_id(pane_id) {
                    let agent_kind = state
                        .context_monitor
                        .pane_states
                        .get(&pane_id)
                        .map(|s| s.agent_kind)
                        .unwrap_or(AgentKind::GenericShell);

                    let cross_insights: Vec<_> = state
                        .context_monitor
                        .pane_states
                        .values()
                        .filter(|s| s.pane_id != pane_id)
                        .flat_map(|s| s.extracted_insights.iter().cloned())
                        .take(crate::context_lifecycle::types::MAX_CROSS_PANE_INSIGHTS)
                        .collect();

                    let pane_name = pane.name.clone();
                    let msg = ContextInjector::build_refresh_message(
                        agent_kind,
                        ContextTier::PostCompaction,
                        &pane_name,
                        &cross_insights,
                    );

                    if pane.write_input(msg.as_bytes()).is_ok() {
                        let _ = pane.write_input(b"\n");
                        if let Some(ps) = state.context_monitor.pane_states.get_mut(&pane_id) {
                            ps.mark_injected();
                        }
                        // Emit compaction notification
                        let bus = state.notification_bus.clone();
                        tokio::runtime::Handle::current().block_on(bus.publish(
                            crate::notification::NotificationEvent::CompactionDetected {
                                pane_id,
                                pane_name: pane_name.clone(),
                            },
                        ));
                        state.mier_activity_feed.push(MierFeedEntry {
                            timestamp: std::time::Instant::now(),
                            kind: MierFeedKind::CompactionDetected,
                            message: format!("Compaction detected → {}", pane_name),
                        });
                    }
                }
            }
        }
    }

    // 4. Clean up monitor state for dead panes
    state.context_monitor.cleanup_dead_panes(&alive_ids);
}
