use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::{state, tooling, validate};

use super::{
    build_tool_context, build_tool_registry, get_session_id, parse_tool_category, print_json,
    refresh_capabilities_manifest, tool_resolution_root,
};

pub fn handle_tooling_list(
    state: &Arc<state::State>,
    impulse_dir: &Path,
    category: Option<String>,
    json: bool,
) -> Result<()> {
    let config = state.config_snapshot()?;
    let registry = build_tool_registry(impulse_dir, &config)?;
    let mut tools = registry.manifest_tools();

    if let Some(ref cat) = category {
        let Some(category_kind) = parse_tool_category(cat) else {
            eprintln!(
                "Unknown category: {} (use: utility, document, analysis, system)",
                cat
            );
            return Ok(());
        };
        let category_name = category_kind.to_string();
        tools.retain(|tool| tool.category == category_name);
    }

    if json {
        print_json(&tools)?;
    } else {
        println!("=== Dynamic Tools ({}) ===\n", tools.len());
        for tool in &tools {
            println!(
                "  {} \u{2014} {} [{} | {}]",
                tool.id, tool.description, tool.category, tool.source
            );
        }
        if tools.is_empty() {
            println!("  (no tools registered)");
        }
        println!("\nUse `tooling-describe <id>` for details.");
    }
    Ok(())
}

pub fn handle_tooling_describe(
    state: &Arc<state::State>,
    impulse_dir: &Path,
    tool_id: String,
    json: bool,
) -> Result<()> {
    validate::validate_tool_id(&tool_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let config = state.config_snapshot()?;
    let registry = build_tool_registry(impulse_dir, &config)?;
    match registry.get(&tool_id) {
        Some(tool) => {
            let desc = tool.descriptor();
            let capabilities: Vec<_> = tool
                .required_capabilities()
                .iter()
                .map(|cap| cap.as_str())
                .collect();
            let source = registry
                .source(&tool_id)
                .map(|value| value.as_str())
                .unwrap_or("builtin");
            if json {
                print_json(&serde_json::json!({
                    "descriptor": desc,
                    "capabilities": capabilities,
                    "source": source,
                }))?;
            } else {
                println!("=== {} (v{}) ===\n", desc.name, desc.version);
                println!("ID:       {}", desc.id);
                println!("Category: {}", desc.category);
                println!("Source:   {}", source);
                println!("Description: {}\n", desc.description);

                if !desc.params.is_empty() {
                    println!("Parameters:");
                    for p in &desc.params {
                        let req = if p.required { "required" } else { "optional" };
                        println!(
                            "  --{} ({:?}, {}) \u{2014} {}",
                            p.name, p.param_type, req, p.description
                        );
                    }
                } else {
                    println!("Parameters: none");
                }

                if !capabilities.is_empty() {
                    println!("\nRequired capabilities: {}", capabilities.join(", "));
                }
            }
        }
        None => {
            eprintln!("Tool not found: {}", tool_id);
            eprintln!("Use `tooling-list` to see available tools.");
        }
    }
    Ok(())
}

pub async fn handle_tooling_run(
    state: &Arc<state::State>,
    impulse_dir: &Path,
    tool_id: String,
    params: Option<String>,
    json: bool,
) -> Result<()> {
    validate::validate_tool_id(&tool_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let config = state.config_snapshot()?;
    let registry = build_tool_registry(impulse_dir, &config)?;
    let params_value: serde_json::Value = if let Some(ref p) = params {
        match serde_json::from_str(p) {
            Ok(value) => value,
            Err(e) => {
                eprintln!("Invalid JSON params: {}", e);
                return Ok(());
            }
        }
    } else {
        serde_json::json!({})
    };

    let ctx = build_tool_context(
        impulse_dir,
        &config,
        tooling::ExecutionOrigin::Cli,
        true,
        get_session_id(None),
    );

    match registry.execute(&tool_id, params_value, &ctx).await {
        Ok(result) => {
            if json {
                print_json(&serde_json::json!({
                    "tool": tool_id,
                    "output": result.output,
                    "artifacts": result.artifacts,
                    "metadata": result.metadata,
                }))?;
            } else {
                print_json(&result.output)?;
                if !result.artifacts.is_empty() {
                    println!("\n--- Artifacts ---");
                    for artifact in &result.artifacts {
                        println!("  {}", artifact.display());
                    }
                }
                if !result.metadata.is_empty() {
                    println!("\n--- Metadata ---");
                    for (k, v) in &result.metadata {
                        println!("  {}: {}", k, v);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Tool execution failed: {}", e);
        }
    }
    Ok(())
}

pub fn handle_tooling_schema(
    state: &Arc<state::State>,
    impulse_dir: &Path,
    format: String,
) -> Result<()> {
    let config = state.config_snapshot()?;
    let registry = build_tool_registry(impulse_dir, &config)?;

    match format.as_str() {
        "json" => {
            print_json(&registry.schema_json())?;
        }
        "markdown" => {
            println!("# Impulse Dynamic Tools\n");
            println!("{}", registry.schema_markdown());
        }
        _ => {
            eprintln!("Unknown format: {} (use: json, markdown)", format);
        }
    }
    Ok(())
}

pub fn handle_tooling_validate(
    state: &Arc<state::State>,
    impulse_dir: &Path,
    json: bool,
) -> Result<()> {
    let config = state.config_snapshot()?;
    let external_tools_dir =
        config.resolved_external_tools_dir_from(tool_resolution_root(impulse_dir));
    let report = tooling::validate_manifests_in_dir(&external_tools_dir);

    if json {
        print_json(&report)?;
    } else {
        println!("=== External Tool Validation ===\n");
        println!("Directory: {}", external_tools_dir.display());
        println!("Valid tools: {}", report.valid_tools);
        println!("Invalid tools: {}", report.invalid_tools);
        if report.issues.is_empty() {
            println!("\nNo validation issues found.");
        } else {
            println!("\nIssues:");
            for issue in &report.issues {
                println!("  {} \u{2014} {}", issue.file, issue.error);
            }
        }
    }

    if report.invalid_tools > 0 {
        anyhow::bail!(
            "found {} invalid external tool manifest(s)",
            report.invalid_tools
        );
    }
    Ok(())
}

pub fn handle_tooling_reload(
    state: &Arc<state::State>,
    impulse_dir: &Path,
    json: bool,
) -> Result<()> {
    let config = state.config_snapshot()?;
    let external_tools_dir =
        config.resolved_external_tools_dir_from(tool_resolution_root(impulse_dir));
    let report = tooling::validate_manifests_in_dir(&external_tools_dir);
    if report.invalid_tools > 0 {
        if json {
            print_json(&report)?;
        }
        anyhow::bail!(
            "cannot reload tooling: found {} invalid external tool manifest(s)",
            report.invalid_tools
        );
    }

    let registry = build_tool_registry(impulse_dir, &config)?;
    let manifest_path = refresh_capabilities_manifest(state.storage().base_path(), &registry)?;

    if json {
        print_json(&serde_json::json!({
            "manifest_path": manifest_path,
            "tool_count": registry.len(),
            "external_tools_dir": external_tools_dir,
        }))?;
    } else {
        println!("Reloaded runtime tooling.");
        println!("External tools dir: {}", external_tools_dir.display());
        println!("Registered tools: {}", registry.len());
        println!("Capabilities manifest: {}", manifest_path.display());
    }
    Ok(())
}
