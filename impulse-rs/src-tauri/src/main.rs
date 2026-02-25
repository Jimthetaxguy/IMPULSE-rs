//! Impulse Tauri Application
//! Entry point for the macOS application wrapper

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Command;
use tauri::Manager;

/// Check if a CLI agent command is available on PATH
#[tauri::command]
fn check_agent(command: String) -> bool {
    Command::new("which")
        .arg(&command)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Launch the Impulse TUI in a new terminal window
#[tauri::command]
fn launch_tui() -> Result<String, String> {
    // Find impulse-rs binary
    let impulse_path = Command::new("which")
        .arg("impulse-rs")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "impulse-rs".to_string());

    // Open in Terminal.app via AppleScript
    let script = format!(
        r#"tell application "Terminal"
            activate
            do script "{} run"
        end tell"#,
        impulse_path
    );

    Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map(|_| "TUI launched in Terminal.app".to_string())
        .map_err(|e| format!("Failed to launch: {}", e))
}

/// Open a new terminal window
#[tauri::command]
fn open_terminal() -> Result<String, String> {
    let script = r#"tell application "Terminal"
        activate
        do script ""
    end tell"#;

    Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map(|_| "Terminal opened".to_string())
        .map_err(|e| format!("Failed to open terminal: {}", e))
}

/// Get impulse-rs status
#[tauri::command]
fn get_status() -> String {
    match Command::new("impulse-rs").arg("status").output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.success() {
                stdout.trim().to_string()
            } else {
                format!("Error: {}", stderr.trim())
            }
        }
        Err(_) => "impulse-rs not found on PATH".to_string(),
    }
}

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Starting Impulse Tauri application");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            check_agent,
            launch_tui,
            open_terminal,
            get_status,
        ])
        .setup(|app| {
            tracing::info!("Impulse app setup complete");

            // Get the main window
            if let Some(window) = app.get_webview_window("main") {
                tracing::info!("Main window created successfully");
                let _ = window.set_title("Impulse - AI Coding Agent Sidecar");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            tracing::error!("Failed to start Tauri application: {}", e);
            eprintln!("ERROR: Failed to start Impulse: {}", e);
            std::process::exit(1);
        });
}
