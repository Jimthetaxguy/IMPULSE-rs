#!/usr/bin/env python3
"""
Impulse Knowledge Base Extractor

Parses all markdown documentation files and populates the SQLite knowledge base.

Usage:
    python3 extract.py                   # Build/rebuild knowledge.db from docs/
    python3 extract.py --query "hooks"   # Quick search after building

Extracts:
    - Document metadata (frontmatter: status, phase, audience, tags)
    - Section structure (## headings with content)
    - ADR decisions (structured Context/Decision/Consequences)
    - Cross-references (markdown links between documents)
    - Concepts (technologies, patterns, tools mentioned)
    - Findings (from ralph loop critique sessions)
    - Risks and assumptions
"""

from __future__ import annotations

import sqlite3
import os
import re
import sys
import json
from pathlib import Path
from datetime import datetime
from typing import Optional

# ============================================================================
# Configuration
# ============================================================================

PROJECT_ROOT = Path(__file__).parent.parent
DOCS_DIR = PROJECT_ROOT / "docs"
DB_PATH = PROJECT_ROOT / "memory-pipeline" / "knowledge.db"
SCHEMA_PATH = PROJECT_ROOT / "memory-pipeline" / "schema.sql"

# Known concept categories
KNOWN_CONCEPTS = {
    # Technologies
    "TypeScript": "technology", "Bun": "technology", "SQLite": "technology",
    "FTS5": "technology", "Vitest": "technology", "Zod": "technology",
    "Claude Code": "technology", "OpenCode": "technology", "Zellij": "technology",
    "PostgreSQL": "technology", "pgvector": "technology", "Prisma": "technology",
    "Node.js": "technology", "React": "technology", "Next.js": "technology",
    "Rust": "technology", "Go": "technology", "Python": "technology",
    "WASM": "technology", "WebAssembly": "technology",
    "mem0": "technology", "sqlite-vec": "technology", "Neo4j": "technology",
    "Qdrant": "technology", "Chroma": "technology", "Faiss": "technology",
    "LiteLLM": "technology", "Ollama": "technology",
    "Anthropic": "technology", "OpenAI": "technology",
    "npm": "technology", "Homebrew": "technology",
    "DOMPurify": "technology", "bcrypt": "technology",
    "Vercel AI SDK": "technology", "Supabase": "technology",
    "Tailwind CSS": "technology", "Zustand": "technology",
    "Playwright": "technology", "GitHub Actions": "technology",
    "DJB2": "technology", "HMAC-SHA256": "technology",
    "CryptoGuard": "technology", "Nx": "technology",
    "pnpm": "technology", "TF-IDF": "technology",
    "JSONL": "technology", "MCP": "technology",
    "Markdown": "technology", "mise": "technology",

    # Patterns
    "Result<T>": "pattern", "atomic writes": "pattern",
    "graceful degradation": "pattern", "deferred extraction": "pattern",
    "few-shot examples": "pattern", "JSON response format": "pattern",
    "file-path matching": "pattern", "40-character fingerprint": "pattern",
    "beginning+end sampling": "pattern", "contradiction flagging": "pattern",
    "temp file + rename": "pattern", "union merge strategy": "pattern",
    "three-file model": "pattern", "hooks architecture": "pattern",
    "fire-and-forget": "pattern", "privacy-aware router": "pattern",
    "speculative drafting": "pattern", "config-driven routing": "pattern",
    "factory functions": "pattern", "test-first": "pattern",
    "structured logging": "pattern", "PII tokenization": "pattern",

    # Principles
    "file-first memory": "principle", "progressive search": "principle",
    "atomic writes": "principle", "graceful degradation": "principle",
    "Engineering Discipline": "principle",

    # Agents / Systems
    "impulse": "agent",

    # Hooks
    "SessionStart": "hook", "SessionEnd": "hook",
    "PostToolUse": "hook", "PreCompact": "hook",
    "PreToolUse": "hook",

    # Files
    "GENOME.md": "file", "LIVE_STATE.json": "file",
    "HISTORY_INDEX.md": "file", "CLAUDE.md": "file",
}

# ============================================================================
# Frontmatter Parser
# ============================================================================

def parse_frontmatter(content: str) -> tuple[dict, str]:
    """Extract YAML frontmatter from markdown content."""
    if not content.startswith("---"):
        return {}, content

    end = content.find("---", 3)
    if end == -1:
        return {}, content

    frontmatter_text = content[3:end].strip()
    body = content[end + 3:].strip()

    # Simple YAML parser (avoids pyyaml dependency)
    meta = {}
    for line in frontmatter_text.split("\n"):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if ":" in line:
            key, _, value = line.partition(":")
            key = key.strip()
            value = value.strip()

            # Handle arrays like [tag1, tag2, tag3]
            if value.startswith("[") and value.endswith("]"):
                items = [item.strip().strip("'\"") for item in value[1:-1].split(",")]
                meta[key] = items
            else:
                meta[key] = value.strip("'\"")

    return meta, body


# ============================================================================
# Section Parser
# ============================================================================

def parse_sections(content: str) -> list[dict]:
    """Extract sections (headings + content) from markdown."""
    sections = []
    lines = content.split("\n")
    current_heading = None
    current_level = 0
    current_content = []
    position = 0

    for line in lines:
        heading_match = re.match(r'^(#{1,6})\s+(.+)$', line)
        if heading_match:
            # Save previous section
            if current_heading is not None:
                text = "\n".join(current_content).strip()
                if text:
                    sections.append({
                        "heading": current_heading,
                        "level": current_level,
                        "content": text,
                        "position": position,
                        "word_count": len(text.split()),
                    })
                    position += 1

            current_level = len(heading_match.group(1))
            current_heading = heading_match.group(2).strip()
            current_content = []
        else:
            current_content.append(line)

    # Don't forget the last section
    if current_heading is not None:
        text = "\n".join(current_content).strip()
        if text:
            sections.append({
                "heading": current_heading,
                "level": current_level,
                "content": text,
                "position": position,
                "word_count": len(text.split()),
            })

    return sections


# ============================================================================
# ADR Decision Parser
# ============================================================================

def parse_adr(content: str, meta: dict) -> dict | None:
    """Extract structured decision from an ADR document."""
    # Check if this looks like an ADR
    adr_match = re.search(r'ADR-?(\d+)', content)
    if not adr_match:
        return None

    adr_number = int(adr_match.group(1))

    # Extract sections by heading
    sections = {}
    current_section = None
    current_lines = []

    for line in content.split("\n"):
        heading_match = re.match(r'^##\s+(.+)$', line)
        if heading_match:
            if current_section:
                sections[current_section] = "\n".join(current_lines).strip()
            current_section = heading_match.group(1).strip()
            current_lines = []
        elif current_section:
            current_lines.append(line)

    if current_section:
        sections[current_section] = "\n".join(current_lines).strip()

    # Extract title from H1
    title_match = re.search(r'^#\s+(?:ADR-?\d+:?\s*)?(.+)$', content, re.MULTILINE)
    title = title_match.group(1).strip() if title_match else "Unknown"

    # Extract consequences
    positive = ""
    negative = ""
    consequences = sections.get("Consequences", "")
    if consequences:
        pos_match = re.search(r'###\s+Positive\s*\n(.*?)(?=###|$)', consequences, re.DOTALL)
        neg_match = re.search(r'###\s+Negative\s*\n(.*?)(?=###|$)', consequences, re.DOTALL)
        if pos_match:
            positive = pos_match.group(1).strip()
        if neg_match:
            negative = neg_match.group(1).strip()

    # Supersedes
    supersedes = None
    supersedes_match = re.search(r'\*\*Supersedes:\*\*\s*\[(.+?)\]', content)
    if supersedes_match:
        supersedes = supersedes_match.group(1)

    return {
        "adr_number": adr_number,
        "title": title,
        "status": meta.get("status", "accepted"),
        "date": meta.get("last_updated", ""),
        "context": sections.get("Context", ""),
        "decision": sections.get("Decision", ""),
        "consequences_positive": positive,
        "consequences_negative": negative,
        "alternatives": sections.get("Alternatives Considered", ""),
        "supersedes": supersedes,
    }


# ============================================================================
# Cross-Reference Extractor
# ============================================================================

def extract_cross_references(content: str) -> list[dict]:
    """Extract markdown links that reference other docs."""
    refs = []
    # Match [text](path.md) patterns
    link_pattern = re.compile(r'\[([^\]]+)\]\(([^)]+\.md[^)]*)\)')

    for match in link_pattern.finditer(content):
        link_text = match.group(1)
        target_path = match.group(2)

        # Get surrounding context (50 chars before and after)
        start = max(0, match.start() - 50)
        end = min(len(content), match.end() + 50)
        context = content[start:end].replace("\n", " ").strip()

        refs.append({
            "target_path": target_path,
            "link_text": link_text,
            "context": context,
        })

    return refs


# ============================================================================
# Concept Extractor
# ============================================================================

def extract_concepts(content: str) -> list[str]:
    """Find known concepts mentioned in content."""
    found = []
    for concept_name in KNOWN_CONCEPTS:
        # Case-sensitive match for most; case-insensitive for longer phrases
        if len(concept_name) <= 3:
            if concept_name in content:
                found.append(concept_name)
        else:
            if concept_name.lower() in content.lower():
                found.append(concept_name)
    return found


# ============================================================================
# Findings Extractor (from ralph loop session logs)
# ============================================================================

def extract_findings(content: str, session: str) -> list[dict]:
    """Extract critique findings from ralph loop session logs."""
    findings = []

    # Look for severity-tagged findings
    # Pattern: **CRITICAL**, **HIGH**, **MEDIUM**, **LOW** followed by description
    severity_pattern = re.compile(
        r'\*\*(CRITICAL|HIGH|MEDIUM|LOW)\*\*[:\s]+(.+?)(?=\n\n|\*\*(?:CRITICAL|HIGH|MEDIUM|LOW)\*\*|\Z)',
        re.DOTALL
    )

    for match in severity_pattern.finditer(content):
        severity = match.group(1)
        desc = match.group(2).strip()
        if len(desc) > 10:  # Filter noise
            findings.append({
                "session": session,
                "severity": severity,
                "title": desc[:100],
                "description": desc,
            })

    # Also look for numbered findings like "Finding 1:" or "1. **"
    numbered_pattern = re.compile(r'(?:Finding\s+)?(\d+)[\.:]\s*\*\*(.+?)\*\*\s*[-—:]+\s*(.+?)(?=\n\n|\d+[\.:]\s*\*\*|\Z)', re.DOTALL)
    for match in numbered_pattern.finditer(content):
        title = match.group(2).strip()
        desc = match.group(3).strip()
        if len(desc) > 10 and title not in [f["title"] for f in findings]:
            findings.append({
                "session": session,
                "severity": "MEDIUM",
                "title": title,
                "description": desc,
            })

    return findings


# ============================================================================
# Risk Extractor
# ============================================================================

def extract_risks(content: str) -> list[dict]:
    """Extract risks and assumptions from document content."""
    risks = []

    # Look for assumption patterns
    assumption_pattern = re.compile(
        r'(?:Assumption|ASSUMPTION|Risk|RISK)\s*[A-Z]?[:\s]+(.+?)(?=\n\n(?:Assumption|ASSUMPTION|Risk|RISK|---)|$)',
        re.DOTALL
    )

    for match in assumption_pattern.finditer(content):
        desc = match.group(1).strip()
        if len(desc) > 20:
            # Try to find severity
            severity = "MEDIUM"
            if "CRITICAL" in desc.upper() or "critical" in desc.lower():
                severity = "CRITICAL"
            elif "HIGH" in desc.upper():
                severity = "HIGH"
            elif "LOW" in desc.upper():
                severity = "LOW"

            risks.append({
                "description": desc[:500],
                "severity": severity,
                "status": "open",
            })

    # Look for table-based risks (| severity | component | description | mitigation |)
    risk_table_pattern = re.compile(
        r'\|\s*(CRITICAL|HIGH|MEDIUM|LOW)\s*\|\s*(.+?)\s*\|\s*(.+?)\s*\|',
        re.MULTILINE
    )
    for match in risk_table_pattern.finditer(content):
        severity = match.group(1)
        component = match.group(2).strip()
        mitigation = match.group(3).strip()
        # Skip table headers
        if component.startswith("-") or component == "Component" or component == "Risk":
            continue
        if len(component) > 3:
            risks.append({
                "description": component,
                "severity": severity,
                "status": "open",
                "mitigation": mitigation,
            })

    # Deduplicate risks by checking first 60 chars of description
    seen = set()
    deduped = []
    for risk in risks:
        key = risk["description"][:60].lower()
        if key not in seen:
            seen.add(key)
            deduped.append(risk)
    risks = deduped

    return risks


# ============================================================================
# Category Classifier
# ============================================================================

def classify_category(path: str) -> str:
    """Determine document category from file path."""
    parts = Path(path).parts
    # Archive check must come before specific subdirectory checks
    if "archive" in parts:
        return "archive"
    elif "decisions" in parts:
        return "decision"
    elif "guides" in parts:
        return "guide"
    elif "phases" in parts:
        return "phase"
    elif "research" in parts:
        return "research"
    elif "spec" in parts:
        return "spec"
    elif "vision" in parts:
        return "vision"
    elif "session-logs" in parts:
        return "session-log"
    elif "archive" in parts:
        return "archive"
    else:
        return "other"


# ============================================================================
# Main Extraction Pipeline
# ============================================================================

def build_database():
    """Build the knowledge database from all documentation files."""
    print(f"Building knowledge database at {DB_PATH}")
    print(f"Scanning {DOCS_DIR}")

    # Remove existing DB and create fresh
    if DB_PATH.exists():
        DB_PATH.unlink()

    conn = sqlite3.connect(str(DB_PATH))
    conn.execute("PRAGMA journal_mode = WAL")
    conn.execute("PRAGMA foreign_keys = ON")

    # Load and execute schema
    with open(SCHEMA_PATH) as f:
        conn.executescript(f.read())

    # Find all markdown files
    md_files = sorted(DOCS_DIR.rglob("*.md"))
    print(f"Found {len(md_files)} markdown files")

    # Also include CLAUDE.md and HONEST-ROADMAP.md from project root if they exist
    for extra in ["CLAUDE.md", "HONEST-ROADMAP.md"]:
        extra_path = PROJECT_ROOT / extra
        if extra_path.exists() and extra_path not in md_files:
            md_files.append(extra_path)

    # Track all concepts for batch insert
    all_concepts = {}  # name -> category
    concept_mentions = []  # (concept_name, doc_id, section_id, context)

    # Process each file
    for md_file in md_files:
        rel_path = str(md_file.relative_to(PROJECT_ROOT))
        print(f"  Processing: {rel_path}")

        content = md_file.read_text(encoding="utf-8")
        meta, body = parse_frontmatter(content)

        # Extract title from first H1
        title_match = re.search(r'^#\s+(.+)$', body, re.MULTILINE)
        title = title_match.group(1).strip() if title_match else md_file.stem

        category = classify_category(rel_path)
        word_count = len(body.split())
        line_count = len(body.split("\n"))

        # Insert document
        cursor = conn.execute(
            """INSERT INTO documents (path, title, category, status, phase, audience, content, word_count, line_count, last_updated)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                rel_path,
                title,
                category,
                meta.get("status", "active"),
                str(meta.get("phase", "")),
                meta.get("audience", "builder"),
                content,
                word_count,
                line_count,
                meta.get("last_updated", ""),
            ),
        )
        doc_id = cursor.lastrowid

        # Insert tags
        tags = meta.get("tags", [])
        if isinstance(tags, str):
            tags = [tags]
        for tag_name in tags:
            tag_name = tag_name.strip()
            if not tag_name:
                continue
            conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?)", (tag_name,))
            tag_row = conn.execute("SELECT id FROM tags WHERE name = ?", (tag_name,)).fetchone()
            if tag_row:
                conn.execute(
                    "INSERT OR IGNORE INTO document_tags (document_id, tag_id) VALUES (?, ?)",
                    (doc_id, tag_row[0]),
                )

        # Insert sections
        sections = parse_sections(body)
        section_ids = {}  # heading -> section_id
        for section in sections:
            cursor = conn.execute(
                """INSERT INTO sections (document_id, heading, level, content, position, word_count)
                   VALUES (?, ?, ?, ?, ?, ?)""",
                (doc_id, section["heading"], section["level"], section["content"],
                 section["position"], section["word_count"]),
            )
            section_ids[section["heading"]] = cursor.lastrowid

        # Parse ADR if it's a decision document (active or archived)
        is_adr = category == "decision" or (category == "archive" and "decisions" in rel_path)
        if is_adr:
            adr = parse_adr(content, meta)
            if adr:
                # Archive decisions are superseded unless explicitly stated otherwise
                if category == "archive" and adr["status"] == "accepted":
                    adr["status"] = "superseded"
                conn.execute(
                    """INSERT INTO decisions (document_id, adr_number, title, status, date,
                       context, decision, consequences_positive, consequences_negative,
                       alternatives, supersedes)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    (doc_id, adr["adr_number"], adr["title"], adr["status"], adr["date"],
                     adr["context"], adr["decision"], adr["consequences_positive"],
                     adr["consequences_negative"], adr["alternatives"], adr["supersedes"]),
                )

        # Extract cross-references
        refs = extract_cross_references(content)
        for ref in refs:
            conn.execute(
                """INSERT INTO cross_references (source_doc_id, target_path, link_text, context)
                   VALUES (?, ?, ?, ?)""",
                (doc_id, ref["target_path"], ref["link_text"], ref["context"]),
            )

        # Extract concepts at document level
        found_concepts = extract_concepts(content)
        for concept_name in found_concepts:
            cat = KNOWN_CONCEPTS.get(concept_name, "other")
            all_concepts[concept_name] = cat
            concept_mentions.append((concept_name, doc_id, None, ""))

        # Extract concepts at section level for finer-grained tracking
        for section in sections:
            section_concepts = extract_concepts(section["content"])
            sec_id = section_ids.get(section["heading"])
            for concept_name in section_concepts:
                cat = KNOWN_CONCEPTS.get(concept_name, "other")
                all_concepts[concept_name] = cat
                concept_mentions.append((concept_name, doc_id, sec_id, section["heading"][:100]))

        # Extract findings from session logs
        if category == "session-log" and "ralph-loop" in rel_path:
            session_match = re.search(r's(\d+)', rel_path)
            session = f"Session {session_match.group(1)}" if session_match else "Unknown"
            findings = extract_findings(content, session)
            for finding in findings:
                conn.execute(
                    """INSERT INTO findings (document_id, session, severity, title, description)
                       VALUES (?, ?, ?, ?, ?)""",
                    (doc_id, finding["session"], finding["severity"],
                     finding["title"], finding["description"]),
                )

        # Extract risks from HONEST-ROADMAP and phase docs
        if "HONEST" in rel_path or "PHASE" in rel_path or category == "phase":
            risks = extract_risks(content)
            for risk in risks:
                conn.execute(
                    """INSERT INTO risks (document_id, description, severity, status, mitigation)
                       VALUES (?, ?, ?, ?, ?)""",
                    (doc_id, risk["description"], risk["severity"],
                     risk.get("status", "open"), risk.get("mitigation", "")),
                )

    # Batch insert concepts
    for concept_name, concept_category in all_concepts.items():
        conn.execute(
            "INSERT OR IGNORE INTO concepts (name, category) VALUES (?, ?)",
            (concept_name, concept_category),
        )

    # Batch insert concept mentions
    for concept_name, doc_id, section_id, context in concept_mentions:
        concept_row = conn.execute(
            "SELECT id FROM concepts WHERE name = ?", (concept_name,)
        ).fetchone()
        if concept_row:
            conn.execute(
                "INSERT OR IGNORE INTO concept_mentions (concept_id, document_id, section_id, context) VALUES (?, ?, ?, ?)",
                (concept_row[0], doc_id, section_id, context),
            )

    # Resolve cross-reference target_doc_ids where possible
    for ref_row in conn.execute("SELECT id, target_path FROM cross_references").fetchall():
        ref_id, target_path = ref_row
        # Normalize the path
        normalized = target_path.lstrip("./").replace("../", "")
        # Try to find matching document
        target_doc = conn.execute(
            "SELECT id FROM documents WHERE path LIKE ?", (f"%{normalized}",)
        ).fetchone()
        if target_doc:
            conn.execute(
                "UPDATE cross_references SET target_doc_id = ? WHERE id = ?",
                (target_doc[0], ref_id),
            )

    conn.commit()

    # Print summary
    print("\n" + "=" * 60)
    print("Knowledge Base Build Summary")
    print("=" * 60)

    for table, label in [
        ("documents", "Documents indexed"),
        ("sections", "Sections extracted"),
        ("concepts", "Concepts identified"),
        ("concept_mentions", "Concept mentions"),
        ("decisions", "ADR decisions"),
        ("findings", "Findings extracted"),
        ("risks", "Risks identified"),
        ("tags", "Unique tags"),
        ("cross_references", "Cross-references"),
    ]:
        count = conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
        print(f"  {label}: {count}")

    total_words = conn.execute("SELECT SUM(word_count) FROM documents").fetchone()[0]
    print(f"  Total words indexed: {total_words:,}")
    print(f"\nDatabase: {DB_PATH}")
    print(f"Size: {DB_PATH.stat().st_size / 1024:.1f} KB")

    conn.close()


# ============================================================================
# Quick Query Interface
# ============================================================================

def query(search_term: str):
    """Run a quick FTS5 search against the knowledge base."""
    if not DB_PATH.exists():
        print("Knowledge database not found. Run: python3 extract.py")
        return

    conn = sqlite3.connect(str(DB_PATH))

    print(f"\nSearching for: {search_term}")
    print("=" * 60)

    # Search documents
    rows = conn.execute(
        """SELECT d.category, d.title, d.path,
                  snippet(documents_fts, 1, '>>>', '<<<', '...', 30) AS snippet
           FROM documents_fts
           JOIN documents d ON d.id = documents_fts.rowid
           WHERE documents_fts MATCH ?
           ORDER BY rank
           LIMIT 10""",
        (search_term,),
    ).fetchall()

    if rows:
        print(f"\nDocuments ({len(rows)} results):")
        for cat, title, path, snippet in rows:
            print(f"  [{cat}] {title}")
            print(f"    {path}")
            print(f"    {snippet}")
            print()

    # Search decisions
    rows = conn.execute(
        """SELECT d.title, d.status,
                  snippet(decisions_fts, 2, '>>>', '<<<', '...', 30) AS snippet
           FROM decisions_fts
           JOIN decisions d ON d.id = decisions_fts.rowid
           WHERE decisions_fts MATCH ?
           ORDER BY rank
           LIMIT 5""",
        (search_term,),
    ).fetchall()

    if rows:
        print(f"\nDecisions ({len(rows)} results):")
        for title, status, snippet in rows:
            print(f"  [{status}] {title}")
            print(f"    {snippet}")
            print()

    # Search concepts
    rows = conn.execute(
        """SELECT c.name, c.category,
                  snippet(concepts_fts, 1, '>>>', '<<<', '...', 30) AS snippet
           FROM concepts_fts
           JOIN concepts c ON c.id = concepts_fts.rowid
           WHERE concepts_fts MATCH ?
           ORDER BY rank
           LIMIT 5""",
        (search_term,),
    ).fetchall()

    if rows:
        print(f"\nConcepts ({len(rows)} results):")
        for name, category, snippet in rows:
            print(f"  [{category}] {name}")
            if snippet:
                print(f"    {snippet}")
            print()

    conn.close()


# ============================================================================
# Entry Point
# ============================================================================

if __name__ == "__main__":
    if "--query" in sys.argv:
        idx = sys.argv.index("--query")
        if idx + 1 < len(sys.argv):
            query(sys.argv[idx + 1])
        else:
            print("Usage: python3 extract.py --query 'search term'")
    else:
        build_database()
