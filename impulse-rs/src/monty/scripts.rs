//! Embedded Python scripts for Monty subprocess integration.
//!
//! Each script receives context via an inline `DATA_JSON` variable
//! (safe JSON from `serde_json::to_string`), uses only Python stdlib
//! (`json`, `re`), and prints a single JSON line to stdout.
//!
//! The scripts are injected into Python code by `build_python_code()`
//! in the parent module.

/// Computed routing script.
///
/// Input JSON: `{context, history_summary, genome_decisions, active_files}`
/// Output JSON: `{target, confidence, reasoning}`
///
/// Weighted scoring across keywords, file extensions, genome signals
/// to pick the best RoutingTarget.
pub const ROUTING_SCRIPT: &str = r#"
import json

data = json.loads(DATA_JSON)
context = data.get("context", "").lower()
history_summary = data.get("history_summary", "").lower()
genome_decisions = [d.lower() for d in data.get("genome_decisions", [])]
active_files = data.get("active_files", [])

scores = {
    "claude-code": 0.0,
    "codex": 0.0,
    "opencode": 0.0,
    "gemini": 0.0,
    "chatgpt": 0.0,
}

# Keyword scoring on context (primary signal)
context_rules = {
    "claude-code": ["architecture", "design", "review", "refactor", "complex", "planning", "debug", "implement"],
    "codex": ["test", "fix", "bug", "lint", "ci", "build", "patch"],
    "opencode": ["opencode", "plugin", "mcp", "server", "hook"],
    "gemini": ["analysis", "research", "strategy", "compare", "evaluate", "benchmark"],
    "chatgpt": ["career", "reflection", "long-term", "narrative", "interview", "story"],
}

for target, keywords in context_rules.items():
    for kw in keywords:
        if kw in context:
            scores[target] += 1.0

# History signal (secondary — boosts if history mentions same domain)
for target, keywords in context_rules.items():
    for kw in keywords:
        if kw in history_summary:
            scores[target] += 0.3

# Genome signal (tertiary — boosts based on decision text)
genome_text = " ".join(genome_decisions)
for target, keywords in context_rules.items():
    for kw in keywords:
        if kw in genome_text:
            scores[target] += 0.2

# File extension signal
ext_map = {
    ".rs": "claude-code",
    ".py": "codex",
    ".ts": "claude-code",
    ".tsx": "claude-code",
    ".js": "codex",
    ".json": "opencode",
    ".md": "chatgpt",
}
for f in active_files:
    for ext, target in ext_map.items():
        if f.endswith(ext):
            scores[target] += 0.5

# Pick winner
best_target = max(scores, key=scores.get)
best_score = scores[best_target]
total = sum(scores.values())

if total == 0:
    confidence = 0.5
    best_target = "codex"
    reasoning = "No signals matched; defaulting to codex"
else:
    confidence = min(best_score / max(total, 1.0), 1.0)
    matched = [k for k, v in scores.items() if v > 0]
    reasoning = f"Scored {best_target}={best_score:.1f} from {len(matched)} active targets"

print(json.dumps({"target": best_target, "confidence": round(confidence, 3), "reasoning": reasoning}))
"#;

/// Injection selection script.
///
/// Input JSON: `{context, history_count, genome_count, active_session}`
/// Output JSON: `[{context_type, priority, reasoning}]`
///
/// Keyword + count analysis produces injection decisions with
/// data-aware priority.
pub const INJECTION_SCRIPT: &str = r#"
import json

data = json.loads(DATA_JSON)
context = data.get("context", "").lower()
history_count = data.get("history_count", 0)
genome_count = data.get("genome_count", 0)
active_session = data.get("active_session", False)

decisions = []

# History injection
if any(kw in context for kw in ["history", "previous", "past", "earlier", "before", "last time"]):
    priority = "high" if history_count > 5 else "medium"
    decisions.append({
        "context_type": "history",
        "priority": priority,
        "reasoning": f"History keywords detected; {history_count} entries available",
    })

# Genome injection
if any(kw in context for kw in ["decision", "preference", "genome", "rule", "constraint", "convention"]):
    priority = "high" if genome_count > 3 else "medium"
    decisions.append({
        "context_type": "genome",
        "priority": priority,
        "reasoning": f"Genome keywords detected; {genome_count} decisions available",
    })

# Session injection
if any(kw in context for kw in ["session", "current", "active", "now", "this"]):
    priority = "high" if active_session else "low"
    decisions.append({
        "context_type": "session",
        "priority": priority,
        "reasoning": f"Session keywords detected; active_session={active_session}",
    })

# Default: inject history at low priority if nothing matched
if not decisions:
    decisions.append({
        "context_type": "history",
        "priority": "low",
        "reasoning": "No specific keywords matched; default injection",
    })

print(json.dumps(decisions))
"#;

/// KDB extraction script.
///
/// Input JSON: `{content, session_id}`
/// Output JSON: `{findings: [], concepts: [], risks: []}`
///
/// Regex extraction of dates/amounts/emails/names, risk detection,
/// concept identification.
pub const EXTRACTION_SCRIPT: &str = r#"
import json
import re

data = json.loads(DATA_JSON)
content = data.get("content", "")
session_id = data.get("session_id", "unknown")

findings = []
concepts = []
risks = []

lower = content.lower()

# Finding extraction
finding_patterns = [
    (r"(?:found|discovered|identified|detected)\s+(.+?)(?:\.|$)", "medium"),
    (r"(?:critical|urgent|severe)\s+(.+?)(?:\.|$)", "high"),
    (r"(?:bug|error|failure|crash)\s+(?:in\s+)?(.+?)(?:\.|$)", "medium"),
]

for pattern, severity in finding_patterns:
    for match in re.finditer(pattern, content, re.IGNORECASE):
        findings.append({
            "content": match.group(0).strip().rstrip("."),
            "severity": severity,
        })

# Concept extraction — capitalized terms, tech terms
concept_patterns = [
    r"\b([A-Z][a-zA-Z]+(?:\s+[A-Z][a-zA-Z]+)+)\b",  # Multi-word proper nouns
    r"\b(API|SDK|CLI|TUI|IPC|JWT|OAuth|REST|gRPC|SQL|NoSQL)\b",  # Tech acronyms
]

seen_concepts = set()
for pattern in concept_patterns:
    for match in re.finditer(pattern, content):
        name = match.group(1).strip()
        if name.lower() not in seen_concepts and len(name) > 2:
            seen_concepts.add(name.lower())
            concepts.append({"name": name})

# Risk extraction
risk_keywords = ["risk", "vulnerability", "concern", "threat", "leak", "exposure",
                 "insecure", "unsafe", "dangerous", "deprecated"]
for kw in risk_keywords:
    if kw in lower:
        # Find the sentence containing the risk keyword
        sentences = re.split(r'[.!?]+', content)
        for sentence in sentences:
            if kw in sentence.lower() and len(sentence.strip()) > 5:
                risks.append({
                    "description": sentence.strip(),
                    "severity": "high" if kw in ("vulnerability", "leak", "exposure", "insecure") else "medium",
                })
                break

print(json.dumps({"findings": findings, "concepts": concepts, "risks": risks}))
"#;

/// Swarm pattern detection script.
///
/// Input JSON: `{agent_a, agent_b, threshold, a_files, b_files, a_tools, b_tools}`
/// Output JSON: `[{pattern_type, confidence, reasoning, file_scope}]`
///
/// File overlap analysis, tool overlap, Echo/Complement/Conflict/Parallel detection.
pub const SWARM_SCRIPT: &str = r#"
import json

data = json.loads(DATA_JSON)
agent_a = data.get("agent_a", "")
agent_b = data.get("agent_b", "")
threshold = data.get("threshold", 0.88)
a_files = set(data.get("a_files", []))
b_files = set(data.get("b_files", []))
a_tools = set(data.get("a_tools", []))
b_tools = set(data.get("b_tools", []))

patterns = []

# Echo detection — same agent or high file overlap
if agent_a.lower() == agent_b.lower():
    patterns.append({
        "pattern_type": "Echo",
        "confidence": 0.95,
        "reasoning": f"Same agent identity: {agent_a}",
        "file_scope": None,
    })
elif a_files and b_files:
    overlap = a_files & b_files
    union = a_files | b_files
    if union:
        overlap_ratio = len(overlap) / len(union)
        if overlap_ratio >= 0.7:
            patterns.append({
                "pattern_type": "Echo",
                "confidence": round(0.6 + overlap_ratio * 0.3, 3),
                "reasoning": f"High file overlap: {len(overlap)}/{len(union)} files shared ({overlap_ratio:.0%})",
                "file_scope": ", ".join(sorted(overlap)[:5]),
            })

# Complement detection — different agents, low file overlap
if agent_a.lower() != agent_b.lower():
    if a_files and b_files:
        overlap = a_files & b_files
        union = a_files | b_files
        overlap_ratio = len(overlap) / len(union) if union else 0
        if overlap_ratio < 0.3:
            conf = 0.8 - overlap_ratio
            if conf >= threshold:
                patterns.append({
                    "pattern_type": "Complement",
                    "confidence": round(conf, 3),
                    "reasoning": f"Low file overlap: {len(overlap)}/{len(union)} files shared ({overlap_ratio:.0%})",
                    "file_scope": None,
                })
    elif not a_files and not b_files:
        # No file data — basic complement based on different identities
        if 0.8 >= threshold:
            patterns.append({
                "pattern_type": "Complement",
                "confidence": 0.8,
                "reasoning": f"Different agents ({agent_a} vs {agent_b}), no file data",
                "file_scope": None,
            })

# Conflict detection — same files, different tools
if a_files and b_files:
    file_overlap = a_files & b_files
    tool_overlap = a_tools & b_tools
    if file_overlap and a_tools and b_tools and not tool_overlap:
        conf = min(len(file_overlap) / max(len(a_files | b_files), 1), 1.0)
        if conf >= threshold:
            patterns.append({
                "pattern_type": "Conflict",
                "confidence": round(conf, 3),
                "reasoning": f"Overlapping files ({len(file_overlap)}) but disjoint tools",
                "file_scope": ", ".join(sorted(file_overlap)[:5]),
            })

# Parallel detection — different files, similar tools
if a_tools and b_tools:
    tool_overlap = a_tools & b_tools
    tool_union = a_tools | b_tools
    tool_ratio = len(tool_overlap) / len(tool_union) if tool_union else 0
    file_overlap_ratio = 0
    if a_files and b_files:
        file_union = a_files | b_files
        file_overlap_ratio = len(a_files & b_files) / len(file_union) if file_union else 0
    if tool_ratio >= 0.5 and file_overlap_ratio < 0.3:
        conf = round(tool_ratio * 0.8, 3)
        if conf >= threshold:
            patterns.append({
                "pattern_type": "Parallel",
                "confidence": conf,
                "reasoning": f"Similar tools ({tool_ratio:.0%} overlap) on different files",
                "file_scope": None,
            })

print(json.dumps(patterns))
"#;
