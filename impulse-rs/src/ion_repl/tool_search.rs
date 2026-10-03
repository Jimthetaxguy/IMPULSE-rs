//! `search_tools`: keyword search over Ion's tool catalog (ADR-0023).
//!
//! Progressive discovery: a search returns names and one-line descriptions,
//! and full input schemas only when asked for, so an agent can find a tool
//! without the whole catalog being pasted into its context.
//!
//! The catalog is a snapshot taken when the tool is registered
//! ([`super::registry::ReplToolRegistry::with_defaults`] registers it last),
//! plus this tool's own entry.

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};

use super::tools::{ReplTool, ToolOutcome};
use super::ReplContext;

/// Default and largest number of matches returned.
pub const DEFAULT_SEARCH_LIMIT: usize = 5;
pub const MAX_SEARCH_LIMIT: usize = 20;

/// One catalog entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub usage: String,
    pub input_schema: Value,
}

impl ToolDescriptor {
    /// Builds a descriptor from a tool's own schema and usage line.
    pub fn of(tool: &dyn ReplTool) -> Self {
        let schema = tool.json_schema();
        Self {
            name: tool.name().to_string(),
            description: schema
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_else(|| tool.usage())
                .to_string(),
            usage: tool.usage().to_string(),
            input_schema: schema.get("input_schema").cloned().unwrap_or(Value::Null),
        }
    }

    /// Relevance of this tool to the query terms. A term in the name counts
    /// most, then a parameter name, then the description or usage line.
    fn score(&self, terms: &[String]) -> u32 {
        let name = self.name.to_lowercase();
        let text = format!("{} {}", self.description, self.usage).to_lowercase();
        let params: Vec<String> = self
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|props| props.keys().map(|key| key.to_lowercase()).collect())
            .unwrap_or_default();
        terms
            .iter()
            .map(|term| {
                let mut score = 0;
                if name.contains(term.as_str()) {
                    score += 4;
                }
                if params.iter().any(|param| param.contains(term.as_str())) {
                    score += 2;
                }
                if text.contains(term.as_str()) {
                    score += 1;
                }
                score
            })
            .sum()
    }
}

pub struct SearchToolsTool {
    catalog: Vec<ToolDescriptor>,
}

impl SearchToolsTool {
    /// A search tool over `catalog`; its own entry is added.
    pub fn new(mut catalog: Vec<ToolDescriptor>) -> Self {
        let mut tool = Self {
            catalog: Vec::new(),
        };
        catalog.push(ToolDescriptor::of(&tool));
        catalog.sort_by(|a, b| a.name.cmp(&b.name));
        tool.catalog = catalog;
        tool
    }

    /// Ranked matches for `query`. A blank query lists every tool.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&ToolDescriptor> {
        let terms: Vec<String> = query
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .filter(|term| !term.is_empty())
            .map(str::to_lowercase)
            .collect();
        if terms.is_empty() {
            return self.catalog.iter().take(limit).collect();
        }
        let mut ranked: Vec<(u32, &ToolDescriptor)> = self
            .catalog
            .iter()
            .map(|tool| (tool.score(&terms), tool))
            .filter(|(score, _)| *score > 0)
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, tool)| tool)
            .collect()
    }
}

#[async_trait]
impl ReplTool for SearchToolsTool {
    fn name(&self) -> &'static str {
        "search_tools"
    }

    fn usage(&self) -> &'static str {
        "search_tools {\"query\": \"...\", \"include_schema\": false, \"limit\": 5} \
         -- find available tools by keyword"
    }

    fn json_schema(&self) -> Value {
        json!({
            "name": "search_tools",
            "description": "Search the available tools by keyword. Returns names and one-line \
                descriptions; set include_schema to also get each match's input schema. An empty \
                query lists every tool.",
            "input_schema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": { "type": "string" },
                    "include_schema": { "type": "boolean" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_SEARCH_LIMIT }
                },
                "required": ["query"]
            }
        })
    }

    async fn run(&self, args: Value, _ctx: &ReplContext) -> Result<ToolOutcome> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .context("search_tools requires a string query")?;
        let include_schema = match args.get("include_schema") {
            None | Some(Value::Null) => false,
            Some(value) => value
                .as_bool()
                .context("search_tools include_schema must be a boolean")?,
        };
        let limit = match args.get("limit") {
            None | Some(Value::Null) => DEFAULT_SEARCH_LIMIT,
            Some(value) => {
                let limit = value
                    .as_u64()
                    .context("search_tools limit must be a positive integer")?;
                anyhow::ensure!(
                    (1..=MAX_SEARCH_LIMIT as u64).contains(&limit),
                    "search_tools limit must be between 1 and {MAX_SEARCH_LIMIT}"
                );
                limit as usize
            }
        };
        let matches = self.search(query, limit);
        let rendered = if matches.is_empty() {
            format!("No tools match {query:?}. Search with an empty query to list them all.")
        } else {
            matches
                .iter()
                .map(|tool| format!("{} -- {}", tool.name, tool.description))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let results: Vec<Value> = matches
            .iter()
            .map(|tool| {
                let mut entry = json!({
                    "name": tool.name,
                    "description": tool.description,
                    "usage": tool.usage,
                });
                if include_schema {
                    entry["input_schema"] = tool.input_schema.clone();
                }
                entry
            })
            .collect();
        let rendered = if include_schema && !results.is_empty() {
            serde_json::to_string_pretty(&results).context("failed to render tool schemas")?
        } else {
            rendered
        };
        Ok(ToolOutcome {
            rendered,
            payload: json!({ "query": query, "results": results }),
            ok: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(name: &str, description: &str, params: &[&str]) -> ToolDescriptor {
        let properties: serde_json::Map<String, Value> = params
            .iter()
            .map(|param| (param.to_string(), json!({"type": "string"})))
            .collect();
        ToolDescriptor {
            name: name.to_string(),
            description: description.to_string(),
            usage: format!("{name} {{...}}"),
            input_schema: json!({"type": "object", "properties": properties}),
        }
    }

    fn tool() -> SearchToolsTool {
        SearchToolsTool::new(vec![
            descriptor("file_read", "Read a file from the workspace", &["path"]),
            descriptor("file_write", "Write a file", &["path", "content"]),
            descriptor("bash_exec", "Run a shell command", &["command"]),
            descriptor("memory_search", "Search GENOME decisions", &["query"]),
        ])
    }

    #[test]
    fn test_search_ranks_name_matches_first() {
        let tool = tool();
        let names: Vec<&str> = tool
            .search("file", 5)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, ["file_read", "file_write"]);
    }

    #[test]
    fn test_search_matches_descriptions_and_parameters() {
        let tool = tool();
        let shell: Vec<&str> = tool
            .search("shell", 5)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(shell, ["bash_exec"]);
        let path = tool.search("path", 5);
        assert!(path.iter().any(|t| t.name == "file_read"));
    }

    #[test]
    fn test_search_blank_query_lists_all_including_itself() {
        let tool = tool();
        let all = tool.search("  ", MAX_SEARCH_LIMIT);
        assert_eq!(all.len(), 5);
        assert!(all.iter().any(|t| t.name == "search_tools"));
    }

    #[test]
    fn test_search_respects_limit_and_reports_no_match() {
        let tool = tool();
        assert_eq!(tool.search("", 2).len(), 2);
        assert!(tool.search("zebra", 5).is_empty());
    }

    #[tokio::test]
    async fn test_run_omits_schema_unless_asked() {
        let tool = tool();
        let ctx = ReplContext::default();
        let brief = tool
            .run(json!({"query": "bash"}), &ctx)
            .await
            .expect("search");
        assert!(brief.payload["results"][0].get("input_schema").is_none());
        assert!(brief.rendered.starts_with("bash_exec -- "));
        let full = tool
            .run(json!({"query": "bash", "include_schema": true}), &ctx)
            .await
            .expect("search");
        assert!(full.payload["results"][0]["input_schema"]["properties"]["command"].is_object());
    }

    #[tokio::test]
    async fn test_run_rejects_bad_arguments() {
        let tool = tool();
        let ctx = ReplContext::default();
        for args in [
            json!({}),
            json!({"query": 3}),
            json!({"query": "x", "limit": 0}),
            json!({"query": "x", "limit": MAX_SEARCH_LIMIT + 1}),
            json!({"query": "x", "include_schema": "yes"}),
        ] {
            assert!(tool.run(args.clone(), &ctx).await.is_err(), "{args}");
        }
    }

    #[tokio::test]
    async fn test_run_no_match_is_ok_with_guidance() {
        let tool = tool();
        let outcome = tool
            .run(json!({"query": "zebra"}), &ReplContext::default())
            .await
            .expect("search");
        assert!(outcome.ok);
        assert!(outcome.rendered.contains("No tools match"));
    }
}
