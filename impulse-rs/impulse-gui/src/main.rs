//! Impulse GUI — Native workbench for AI coding agents.
//!
//! Pure Rust GUI using egui/eframe with embedded terminals via egui_term,
//! plus sidebar navigation to daemon views (Sessions, Genome, Search).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent_panel;
mod app;
mod global_config;
mod identity;
mod ipc;
mod project_context;
mod project_scaffold;
mod state;
mod theme;
mod views;
mod widgets;

use app::ImpulseApp;
use eframe::egui;
use std::env;
use std::process;

fn main() -> eframe::Result {
    let args: Vec<String> = env::args().collect();
    let debug_mode = args.iter().any(|a| a == "--debug" || a == "-d");

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Impulse GUI - Workbench for AI coding agents");
        println!();
        println!("Usage: impulse-gui [OPTIONS]");
        println!();
        println!("Options:");
        println!("  -d, --debug     Enable debug logging");
        println!("  -h, --help      Show this help message");
        println!("  --version       Show version information");
        println!();
        println!("Keyboard shortcuts:");
        println!("  Ctrl+1-4        Switch views (Terminals/Sessions/Genome/Search)");
        println!("  Ctrl+5          Toggle Agent panel");
        println!("  Ctrl+B          Toggle sidebar");
        println!("  Ctrl+T          New terminal tab");
        println!("  Ctrl+W          Close current terminal tab");
        println!("  Ctrl+Tab        Next terminal tab");
        println!("  Ctrl+Shift+Tab  Previous terminal tab");
        println!("  Ctrl+K          Focus search");
        println!("  Ctrl+R          Refresh daemon data");
        process::exit(0);
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Impulse GUI v{}", env!("CARGO_PKG_VERSION"));
        println!("Platform: {} {}", env::consts::OS, env::consts::ARCH);
        process::exit(0);
    }

    setup_panic_handler();

    let log_level = if debug_mode { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .format_timestamp_millis()
        .init();

    log::info!("Impulse GUI starting...");
    log::info!("Version: {}", env!("CARGO_PKG_VERSION"));
    log::info!("Platform: {} {}", env::consts::OS, env::consts::ARCH);
    log_agent_availability();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("Impulse")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Impulse",
        options,
        Box::new(|cc| {
            theme::apply_dark_theme(&cc.egui_ctx);
            Ok(Box::new(ImpulseApp::new(cc)))
        }),
    )
}

fn log_agent_availability() {
    let agents = [
        ("Claude Code", "claude"),
        ("OpenCode", "opencode"),
        ("Codex", "codex"),
    ];

    for (name, cmd) in &agents {
        match which::which(cmd) {
            Ok(path) => log::info!("{}: available at {}", name, path.display()),
            Err(_) => log::warn!("{}: not found on PATH", name),
        }
    }
}

fn setup_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        log::error!("PANIC at {}: {}", location, msg);
        eprintln!("PANIC at {}: {}", location, msg);
    }));
}
