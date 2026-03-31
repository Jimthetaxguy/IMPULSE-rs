//! Direct-mode CLI handlers for plugin commands (plugin-list, plugin-invoke).
//!
//! Extracted from `run_direct_mode()` in main.rs.

use anyhow::Result;

use crate::plugin;

/// Handle `plugin-list` in direct mode (no daemon).
pub fn handle_plugin_list(json: bool) -> Result<()> {
    let registry = plugin::registry::init_global_registry();
    let providers = registry.list_context_providers().unwrap_or_default();
    let handlers = registry.list_action_handlers_metadata().unwrap_or_default();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "context_providers": providers,
                "action_handlers": handlers,
            }))?
        );
    } else {
        println!("Context Providers ({}):", providers.len());
        for p in &providers {
            println!(
                "  {} v{} — {} [{}]",
                p.name,
                p.version,
                p.description,
                p.supported_formats.join(", ")
            );
        }
        println!("\nAction Handlers ({}):", handlers.len());
        for h in &handlers {
            println!("  {} v{} — {}", h.name, h.version, h.description);
        }
        if providers.is_empty() && handlers.is_empty() {
            println!("\nNo plugins registered.");
        }
    }
    Ok(())
}

/// Handle `plugin-invoke` in direct mode (no daemon).
pub fn handle_plugin_invoke(
    name: String,
    path: Option<String>,
    query: Option<String>,
    options: Option<String>,
    json: bool,
) -> Result<()> {
    let registry = plugin::registry::init_global_registry();
    let mut input = plugin::PluginInput::new();
    if let Some(p) = path {
        input = input.with_path(std::path::PathBuf::from(p));
    }
    if let Some(q) = query {
        input = input.with_query(q);
    }
    if let Some(opts) = options {
        let parsed: serde_json::Value =
            serde_json::from_str(&opts).unwrap_or_else(|_| serde_json::json!({"raw": opts}));
        input = input.with_options(parsed);
    }
    match registry.get_action_handler(&name) {
        Ok(Some(handler)) => {
            if let Err(e) = handler.validate(&input) {
                eprintln!("Validation error: {}", e);
                std::process::exit(1);
            }
            match handler.execute(&input) {
                Ok(output) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        println!("{}", output.content);
                    }
                }
                Err(e) => {
                    eprintln!("Plugin error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Ok(None) => {
            eprintln!("Plugin not found: {}", name);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Registry error: {}", e);
            std::process::exit(1);
        }
    }
    Ok(())
}
