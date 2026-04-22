//! Primary keyword-based routing implementation
//!
//! This module provides keyword-based routing for computed routing
//! and injection selection. Future: #[cfg(feature = "monty-support")]
//! enables PyO3 computed routing.

use super::{ComputedRoute, InjectionDecision, RoutingTarget};

/// Route to a target based on keyword analysis of the context
pub fn route_by_keywords(context: &str) -> Result<ComputedRoute, String> {
    let combined = context.to_lowercase();

    let target = if combined.contains("architecture")
        || combined.contains("design")
        || combined.contains("review")
        || combined.contains("refactor")
        || combined.contains("complex")
        || combined.contains("planning")
    {
        RoutingTarget::ClaudeCode
    } else if combined.contains("opencode")
        || combined.contains("plugin")
        || combined.contains("mcp")
    {
        RoutingTarget::OpenCode
    } else if combined.contains("analysis")
        || combined.contains("research")
        || combined.contains("strategy")
    {
        RoutingTarget::Gemini
    } else if combined.contains("career")
        || combined.contains("reflection")
        || combined.contains("long-term")
    {
        RoutingTarget::ChatGPT
    } else {
        RoutingTarget::Codex
    };

    Ok(ComputedRoute {
        target,
        confidence: 0.7,
        reasoning: "Keyword-based routing: matched keywords in context".to_string(),
        functions_called: vec![],
    })
}

/// Select injection contexts based on keyword analysis
pub fn select_injection_by_keywords(context: &str) -> Result<Vec<InjectionDecision>, String> {
    let combined = context.to_lowercase();
    let mut decisions = Vec::new();

    if combined.contains("history") || combined.contains("previous") || combined.contains("past") {
        decisions.push(InjectionDecision {
            context_type: "history".to_string(),
            priority: "medium".to_string(),
            reasoning: "Context contains history-related keywords".to_string(),
        });
    }

    if combined.contains("decision")
        || combined.contains("preference")
        || combined.contains("genome")
    {
        decisions.push(InjectionDecision {
            context_type: "genome".to_string(),
            priority: "medium".to_string(),
            reasoning: "Context contains decision/preference keywords".to_string(),
        });
    }

    if combined.contains("session") || combined.contains("current") || combined.contains("active") {
        decisions.push(InjectionDecision {
            context_type: "session".to_string(),
            priority: "high".to_string(),
            reasoning: "Context contains session-related keywords".to_string(),
        });
    }

    if decisions.is_empty() {
        decisions.push(InjectionDecision {
            context_type: "history".to_string(),
            priority: "low".to_string(),
            reasoning: "Default injection: history".to_string(),
        });
    }

    Ok(decisions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_routing_architecture() {
        let result =
            route_by_keywords("I need to do an architecture review for the auth system").unwrap();
        assert_eq!(result.target, RoutingTarget::ClaudeCode);
    }

    #[test]
    fn test_keyword_routing_opencode() {
        let result = route_by_keywords("Create an opencode plugin for the system").unwrap();
        assert_eq!(result.target, RoutingTarget::OpenCode);
    }

    #[test]
    fn test_keyword_routing_default() {
        let result = route_by_keywords("Fix the failing tests").unwrap();
        assert_eq!(result.target, RoutingTarget::Codex);
    }

    #[test]
    fn test_keyword_injection() {
        let decisions =
            select_injection_by_keywords("Look at previous sessions for similar tasks").unwrap();
        assert!(!decisions.is_empty());
        assert!(decisions.iter().any(|d| d.context_type == "history"));
    }
}
