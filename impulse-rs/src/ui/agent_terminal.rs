use super::*;
use crate::state::Platform;

pub(crate) fn spawn_agent_in_terminal(state: &mut TuiState, agent_cmd: &str, platform: Platform) {
    // Create pane manager if it doesn't exist
    if state.pane_manager.is_none() {
        state.pane_manager = Some(crate::ui::pane_manager::PaneManager::new());
    }

    if let Some(ref mut pm) = state.pane_manager {
        // Get current working directory from first project
        let cwd = state.projects.first().map(|p| p.path.as_path());

        // Generate session ID
        let session_id = format!(
            "{}-{}-{}",
            agent_cmd,
            chrono::Local::now().format("%H%M%S"),
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        // Default PTY size
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };

        // Create the pane
        match pm.create_pane(crate::ui::pane_manager::PaneCreateRequest {
            name: agent_cmd.to_string(),
            command: agent_cmd,
            args: &[],
            working_dir: cwd,
            size,
            project_index: state.active_project_index,
            impulse_home: None,
            scrollback_lines: None,
            session_id: Some(&session_id),
            platform: Some(match platform {
                Platform::ClaudeCode => "Claude Code",
                Platform::Codex => "Codex",
                Platform::OpenCode => "OpenCode",
            }),
        }) {
            Ok(pane_id) => {
                // Also create UI terminal tab for display
                let tab_name = format!("{}-{}", agent_cmd, pane_id);
                let new_tab = TerminalTab {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: tab_name.clone(),
                    session_id: Some(session_id.clone()),
                    platform: Some(platform),
                    is_active: true,
                    last_output: String::new(),
                    pane_id: Some(pane_id),
                };
                state.terminal_tabs.push(new_tab);
                state.active_terminal_tab = state.terminal_tabs.len() - 1;

                // Schedule context lifecycle injection after startup delay
                let agent_kind = AgentKind::detect(agent_cmd, &tab_name);
                state.pending_injections.push(PendingInjection {
                    pane_id,
                    pane_name: tab_name,
                    agent_kind,
                    scheduled_at: std::time::Instant::now(),
                });
                // Register pane in context monitor
                state
                    .context_monitor
                    .pane_states
                    .insert(pane_id, PaneContextState::new(pane_id, agent_kind));

                state.status_message = Some(format!(
                    "Spawned {} with session {}",
                    agent_cmd,
                    &session_id[..session_id.len().min(12)]
                ));
            }
            Err(e) => {
                state.status_message = Some(format!("Failed to spawn {}: {}", agent_cmd, e));
            }
        }
    } else {
        state.status_message = Some("Pane manager unavailable".to_string());
    }
}
