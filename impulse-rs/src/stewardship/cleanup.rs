use chrono::Utc;

use super::types::*;

/// Generate cleanup proposals based on session analysis and threshold level
pub fn generate_proposals(
    analysis: &SessionAnalysis,
    threshold: ThresholdLevel,
) -> Vec<CleanupProposal> {
    let mut proposals = Vec::new();

    match threshold {
        ThresholdLevel::Passive => {
            // No proposals at passive level
        }
        ThresholdLevel::Monitor => {
            // Detect duplicates but don't propose (monitoring only)
            // The analysis already contains duplicate_regions for display
        }
        ThresholdLevel::Surgical => {
            proposals.extend(strategy_deduplicate(analysis));
            proposals.extend(strategy_condense(analysis));
        }
        ThresholdLevel::Thoughtful => {
            proposals.extend(strategy_deduplicate(analysis));
            proposals.extend(strategy_condense(analysis));
            proposals.extend(strategy_remove_rot(analysis));
            proposals.extend(strategy_consolidate(analysis));
        }
        ThresholdLevel::Emergency => {
            proposals.extend(strategy_deduplicate(analysis));
            proposals.extend(strategy_condense(analysis));
            proposals.extend(strategy_remove_rot(analysis));
            proposals.extend(strategy_consolidate(analysis));
            proposals.extend(strategy_emergency_summarize(analysis));
        }
    }

    proposals
}

/// Deduplicate: Collapse repeated identical tool calls (3+ consecutive)
fn strategy_deduplicate(analysis: &SessionAnalysis) -> Vec<CleanupProposal> {
    analysis
        .duplicate_regions
        .iter()
        .map(|region| CleanupProposal {
            id: format!("dedup-{}-{}", analysis.session_id, uuid_short()),
            created_at: Utc::now(),
            session_id: analysis.session_id.clone(),
            threshold: ThresholdLevel::Surgical,
            strategy: CleanupStrategy::Deduplicate,
            estimated_tokens_freed: region
                .estimated_tokens
                .saturating_sub(region.estimated_tokens / region.occurrences),
            regions: vec![ProposalRegion {
                description: format!(
                    "{} duplicate {} calls",
                    region.occurrences, region.tool_name
                ),
                message_indices: region.indices.clone(),
                estimated_tokens: region.estimated_tokens,
            }],
            preserves: vec![format!("First {} call retained", region.tool_name)],
            status: ProposalStatus::Pending,
        })
        .collect()
}

/// Condense: Propose summarizing verbose tool outputs
fn strategy_condense(analysis: &SessionAnalysis) -> Vec<CleanupProposal> {
    // Find tool patterns with high token counts
    let verbose_patterns: Vec<_> = analysis
        .tool_patterns
        .iter()
        .filter(|p| p.count >= 3)
        .collect();

    if verbose_patterns.is_empty() {
        return Vec::new();
    }

    let total_tokens: usize = verbose_patterns.iter().map(|p| p.count * 50).sum(); // rough estimate
    vec![CleanupProposal {
        id: format!("condense-{}-{}", analysis.session_id, uuid_short()),
        created_at: Utc::now(),
        session_id: analysis.session_id.clone(),
        threshold: ThresholdLevel::Surgical,
        strategy: CleanupStrategy::Condense,
        estimated_tokens_freed: total_tokens / 2,
        regions: verbose_patterns
            .iter()
            .map(|p| ProposalRegion {
                description: format!("Condense {} repeated {} calls", p.count, p.tool_name),
                message_indices: vec![p.first_index, p.last_index],
                estimated_tokens: p.count * 50,
            })
            .collect(),
        preserves: vec!["Tool outputs summarized, not removed".to_string()],
        status: ProposalStatus::Pending,
    }]
}

/// Remove rot: Early context superseded by later work
fn strategy_remove_rot(analysis: &SessionAnalysis) -> Vec<CleanupProposal> {
    analysis
        .rot_candidates
        .iter()
        .map(|rot| CleanupProposal {
            id: format!("rot-{}-{}", analysis.session_id, uuid_short()),
            created_at: Utc::now(),
            session_id: analysis.session_id.clone(),
            threshold: ThresholdLevel::Thoughtful,
            strategy: CleanupStrategy::RemoveRot,
            estimated_tokens_freed: rot.estimated_tokens,
            regions: vec![ProposalRegion {
                description: rot.description.clone(),
                message_indices: (rot.message_range.0..rot.message_range.1).collect(),
                estimated_tokens: rot.estimated_tokens,
            }],
            preserves: vec!["Decisions from rot region preserved separately".to_string()],
            status: ProposalStatus::Pending,
        })
        .collect()
}

/// Consolidate: Merge similar contexts
fn strategy_consolidate(analysis: &SessionAnalysis) -> Vec<CleanupProposal> {
    // If many decisions about the same topic, consolidate
    if analysis.decisions.len() < 3 {
        return Vec::new();
    }

    let total_decision_tokens: usize = analysis
        .decisions
        .iter()
        .map(|d| d.description.len() / 4)
        .sum();

    vec![CleanupProposal {
        id: format!("consolidate-{}-{}", analysis.session_id, uuid_short()),
        created_at: Utc::now(),
        session_id: analysis.session_id.clone(),
        threshold: ThresholdLevel::Thoughtful,
        strategy: CleanupStrategy::Consolidate,
        estimated_tokens_freed: total_decision_tokens / 3,
        regions: vec![ProposalRegion {
            description: format!(
                "Consolidate {} decisions into unified summary",
                analysis.decisions.len()
            ),
            message_indices: analysis.decisions.iter().map(|d| d.message_index).collect(),
            estimated_tokens: total_decision_tokens,
        }],
        preserves: vec!["All decision content preserved in consolidated form".to_string()],
        status: ProposalStatus::Pending,
    }]
}

/// Emergency summarize: Aggressive cleanup preserving only critical context
fn strategy_emergency_summarize(analysis: &SessionAnalysis) -> Vec<CleanupProposal> {
    let target_tokens = analysis.estimated_tokens / 2; // Free 50% of context

    vec![CleanupProposal {
        id: format!("emergency-{}-{}", analysis.session_id, uuid_short()),
        created_at: Utc::now(),
        session_id: analysis.session_id.clone(),
        threshold: ThresholdLevel::Emergency,
        strategy: CleanupStrategy::EmergencySummarize,
        estimated_tokens_freed: target_tokens,
        regions: vec![ProposalRegion {
            description: "Full session summarization — preserve decisions, files, next steps only"
                .to_string(),
            message_indices: (0..analysis.message_count).collect(),
            estimated_tokens: analysis.estimated_tokens,
        }],
        preserves: vec![
            "All decisions".to_string(),
            "Files touched".to_string(),
            "Current task context".to_string(),
            "Next steps".to_string(),
        ],
        status: ProposalStatus::Pending,
    }]
}

/// Build a refined context injection block from analysis + cross-project memory
pub fn build_refined_context(
    analysis: &SessionAnalysis,
    cross_project: &CrossProjectMemory,
) -> String {
    let mut lines = Vec::new();
    lines.push("## Stewardship Context (auto-generated)".to_string());
    lines.push(String::new());

    // Session summary
    lines.push(format!(
        "**Session:** {} | **Messages:** {} | **Tokens:** ~{}",
        analysis.session_id, analysis.message_count, analysis.estimated_tokens
    ));
    lines.push(String::new());

    // Decisions
    if !analysis.decisions.is_empty() {
        lines.push("### Key Decisions".to_string());
        for decision in &analysis.decisions {
            lines.push(format!("- {}", decision.description));
        }
        lines.push(String::new());
    }

    // Files touched
    if !analysis.files_touched.is_empty() {
        lines.push("### Files Touched".to_string());
        for file in &analysis.files_touched {
            lines.push(format!("- `{}`", file));
        }
        lines.push(String::new());
    }

    // Cross-project insights
    let relevant: Vec<_> = cross_project.patterns.iter().take(3).collect();
    if !relevant.is_empty() {
        lines.push("### Cross-Project Insights".to_string());
        for pattern in relevant {
            lines.push(format!(
                "- {} (seen {} times)",
                pattern.insight, pattern.occurrences
            ));
        }
        lines.push(String::new());
    }

    // Key insights
    if !analysis.key_insights.is_empty() {
        lines.push("### Session Insights".to_string());
        for insight in &analysis.key_insights {
            lines.push(format!("- {}", insight));
        }
    }

    lines.join("\n")
}

fn uuid_short() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_analysis(tokens: usize, duplicates: usize, decisions: usize) -> SessionAnalysis {
        let mut dup_regions = Vec::new();
        for i in 0..duplicates {
            dup_regions.push(DuplicateRegion {
                tool_name: "Bash".to_string(),
                occurrences: 4,
                indices: vec![i * 4, i * 4 + 1, i * 4 + 2, i * 4 + 3],
                estimated_tokens: 200,
                input_preview: format!("command_{}", i),
            });
        }

        let mut decision_list = Vec::new();
        for i in 0..decisions {
            decision_list.push(ExtractedDecision {
                description: format!("Decision {} about architecture", i),
                context: format!("Context for decision {}", i),
                message_index: i,
            });
        }

        SessionAnalysis {
            session_id: "test-session".to_string(),
            project_hash: "test-hash".to_string(),
            transcript_path: PathBuf::from("/tmp/test.jsonl"),
            analyzed_at: Utc::now(),
            message_count: 50,
            estimated_tokens: tokens,
            estimated_context_pct: tokens as f32 / 200_000.0,
            decisions: decision_list,
            files_touched: vec!["src/main.rs".to_string()],
            tool_patterns: vec![],
            duplicate_regions: dup_regions,
            rot_candidates: vec![],
            key_insights: vec!["Test insight".to_string()],
        }
    }

    #[test]
    fn test_passive_generates_no_proposals() {
        let analysis = make_analysis(10_000, 2, 1);
        let proposals = generate_proposals(&analysis, ThresholdLevel::Passive);
        assert!(proposals.is_empty());
    }

    #[test]
    fn test_monitor_generates_no_proposals() {
        let analysis = make_analysis(60_000, 2, 1);
        let proposals = generate_proposals(&analysis, ThresholdLevel::Monitor);
        assert!(proposals.is_empty());
    }

    #[test]
    fn test_surgical_generates_dedup_proposals() {
        let analysis = make_analysis(90_000, 2, 1);
        let proposals = generate_proposals(&analysis, ThresholdLevel::Surgical);
        assert!(!proposals.is_empty());
        assert!(proposals
            .iter()
            .any(|p| p.strategy == CleanupStrategy::Deduplicate));
    }

    #[test]
    fn test_thoughtful_generates_consolidation() {
        let analysis = make_analysis(120_000, 1, 5);
        let proposals = generate_proposals(&analysis, ThresholdLevel::Thoughtful);
        assert!(proposals
            .iter()
            .any(|p| p.strategy == CleanupStrategy::Consolidate));
    }

    #[test]
    fn test_emergency_generates_summarize() {
        let analysis = make_analysis(160_000, 1, 5);
        let proposals = generate_proposals(&analysis, ThresholdLevel::Emergency);
        assert!(proposals
            .iter()
            .any(|p| p.strategy == CleanupStrategy::EmergencySummarize));
    }

    #[test]
    fn test_build_refined_context() {
        let analysis = make_analysis(10_000, 0, 2);
        let memory = CrossProjectMemory::default();
        let context = build_refined_context(&analysis, &memory);
        assert!(context.contains("Stewardship Context"));
        assert!(context.contains("Key Decisions"));
        assert!(context.contains("Files Touched"));
    }
}
