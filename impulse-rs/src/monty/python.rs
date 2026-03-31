//! PyO3 Monty integration (future)
//!
//! When `monty-support` feature is enabled, this module will provide
//! computed routing via pydantic-monty using pyo3.
//!
//! ## External Functions
//!
//! These functions would be registered with Monty's sandbox:
//!
//! - `route_to(tool_name)` - Route to a specific tool
//! - `search_history(query, limit)` - Search session history
//! - `get_genome_decisions(topic)` - Get genome decisions
//! - `inject(context, priority)` - Mark context for injection
//! - `extract_findings(content)` - Extract findings from content
//! - `search_similar(query, limit)` - Search for similar sessions

/// Descriptions of external functions that a future Monty sandbox can call
#[cfg(test)]
pub fn get_external_functions() -> Vec<(&'static str, &'static str)> {
    vec![
        ("route_to", "Route to a specific tool"),
        ("search_history", "Search session history"),
        ("get_genome_decisions", "Get genome decisions"),
        ("inject", "Mark context for injection"),
        ("extract_findings", "Extract findings from content"),
        ("search_similar", "Search for similar sessions"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_external_functions() {
        let funcs = get_external_functions();
        assert!(!funcs.is_empty());
        assert_eq!(funcs.len(), 6);
    }
}
