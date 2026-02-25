#!/usr/bin/env python3
"""
Impulse Knowledge Base Query Tool

Fast FTS5-powered search across all project documentation.

Query:
    kdb "search terms"                        # Full-text search
    kdb --json "search terms"                 # JSON output (for agents)
    kdb --decisions                            # List active ADR decisions
    kdb --all-decisions                        # Include superseded ADRs
    kdb --risks                                # List open risks
    kdb --concepts                             # List all concepts
    kdb --concepts technology                  # Filter by category
    kdb --stats                                # Database statistics
    kdb --summary                              # Quick project overview
    kdb --phase 1                              # Documents for a phase
    kdb --tag hooks                            # Documents with a tag
    kdb --doc "PRODUCT-SPEC"                   # Find document by name
    kdb --related "PRODUCT-SPEC"               # Find related documents
    kdb --xref "GENOME.md"                     # Cross-references to target

Contribute:
    kdb --add-finding SEV "title" "desc"       # Add a finding (SEV: CRITICAL|HIGH|MEDIUM|LOW)
    kdb --add-risk SEV "description"           # Add a risk
    kdb --add-concept "name" "category"        # Add a concept (category: technology|pattern|principle|etc)
    kdb --log "session-name" "summary"         # Log a session summary

Maintain:
    kdb --check                                # Check if DB is stale (docs changed since last build)
    kdb --rebuild                              # Rebuild from docs/
"""

from __future__ import annotations

import sqlite3
import sys
import json
import re
from pathlib import Path

DB_PATH = Path(__file__).parent / "knowledge.db"


def get_conn():
    if not DB_PATH.exists():
        print("Knowledge database not found. Run: python3 extract.py", file=sys.stderr)
        sys.exit(1)
    return sqlite3.connect(str(DB_PATH))


def escape_fts5(term: str) -> str:
    """Escape a search term for safe FTS5 MATCH queries.

    FTS5 interprets certain characters as operators:
    - Hyphens as NOT
    - AND/OR/NOT as boolean operators
    - Colons as column filters

    We wrap each word in double quotes to treat them as literal strings.
    """
    # If user already used quotes, pass through
    if '"' in term:
        return term
    # Wrap each word in quotes to avoid FTS5 operator interpretation
    words = term.split()
    return " ".join(f'"{w}"' for w in words)


# ============================================================================
# Search Commands
# ============================================================================

def search(term: str, limit: int = 10, as_json: bool = False):
    """Full-text search across all documents."""
    conn = get_conn()
    safe_term = escape_fts5(term)

    results = []

    # Search documents
    rows = conn.execute("""
        SELECT d.category, d.title, d.path, d.status, d.phase,
               snippet(documents_fts, 1, '>>>', '<<<', '...', 30) AS snippet,
               rank
        FROM documents_fts
        JOIN documents d ON d.id = documents_fts.rowid
        WHERE documents_fts MATCH ?
        ORDER BY rank
        LIMIT ?
    """, (safe_term, limit)).fetchall()

    for cat, title, path, status, phase, snippet, rank in rows:
        results.append({
            "type": "document",
            "category": cat,
            "title": title,
            "path": path,
            "status": status,
            "phase": phase,
            "snippet": snippet,
        })

    # Search sections
    rows = conn.execute("""
        SELECT d.category, s.heading, d.path, d.status, d.phase,
               snippet(sections_fts, 1, '>>>', '<<<', '...', 30) AS snippet
        FROM sections_fts
        JOIN sections s ON s.id = sections_fts.rowid
        JOIN documents d ON d.id = s.document_id
        WHERE sections_fts MATCH ?
        ORDER BY rank
        LIMIT ?
    """, (safe_term, limit)).fetchall()

    for cat, heading, path, status, phase, snippet in rows:
        results.append({
            "type": "section",
            "category": cat,
            "title": heading,
            "path": path,
            "status": status,
            "phase": phase,
            "snippet": snippet,
        })

    # Search decisions
    rows = conn.execute("""
        SELECT dec.title, dec.status, dec.adr_number, d.path,
               snippet(decisions_fts, 2, '>>>', '<<<', '...', 30) AS snippet
        FROM decisions_fts
        JOIN decisions dec ON dec.id = decisions_fts.rowid
        LEFT JOIN documents d ON d.id = dec.document_id
        WHERE decisions_fts MATCH ?
        ORDER BY rank
        LIMIT 5
    """, (safe_term,)).fetchall()

    for title, status, adr, path, snippet in rows:
        results.append({
            "type": "decision",
            "category": "decision",
            "title": f"ADR-{adr:04d}: {title}" if adr else title,
            "path": path or "",
            "status": status,
            "snippet": snippet,
        })

    conn.close()

    if as_json:
        print(json.dumps(results, indent=2))
    else:
        print(f"\n=== Search: \"{term}\" ({len(results)} results) ===\n")
        for r in results:
            print(f"  [{r['category']}] {r['title']}")
            print(f"    {r['path']}")
            if r.get("snippet"):
                snippet_clean = r["snippet"].replace(">>>", "\033[1m").replace("<<<", "\033[0m")
                print(f"    {snippet_clean}")
            print()


def list_decisions(include_superseded: bool = False):
    """List ADR decisions."""
    conn = get_conn()

    if include_superseded:
        rows = conn.execute("""
            SELECT d.adr_number, d.title, d.status, d.date, doc.path
            FROM decisions d
            LEFT JOIN documents doc ON doc.id = d.document_id
            ORDER BY d.adr_number, d.status
        """).fetchall()
        label = "All Decisions (including superseded)"
    else:
        rows = conn.execute("SELECT * FROM v_active_decisions").fetchall()
        label = "Active Decisions"

    conn.close()

    print(f"\n=== {label} ===\n")
    for adr, title, status, date, path in rows:
        status_icon = "x" if status == "superseded" else ">"
        print(f"  [{status_icon}] ADR-{adr:04d}: {title}")
        print(f"      Status: {status} | Date: {date or 'N/A'}")
        print(f"      File: {path}")
        print()


def list_risks():
    """List all open risks by severity."""
    conn = get_conn()
    rows = conn.execute("SELECT * FROM v_open_risks").fetchall()
    conn.close()

    print("\n=== Open Risks ===\n")
    for severity, desc, mitigation, phase, path in rows:
        desc_short = desc[:120].replace('\n', ' ')
        print(f"  [{severity}] {desc_short}")
        if mitigation:
            print(f"    Mitigation: {mitigation[:80]}")
        print(f"    Source: {path}")
        print()


def list_concepts(category_filter: str = None):
    """List all identified concepts."""
    conn = get_conn()

    if category_filter:
        rows = conn.execute("""
            SELECT c.name, c.category, COUNT(cm.document_id) AS mentions
            FROM concepts c
            LEFT JOIN concept_mentions cm ON cm.concept_id = c.id
            WHERE c.category = ?
            GROUP BY c.id
            ORDER BY mentions DESC, c.name
        """, (category_filter,)).fetchall()
    else:
        rows = conn.execute("""
            SELECT c.name, c.category, COUNT(cm.document_id) AS mentions
            FROM concepts c
            LEFT JOIN concept_mentions cm ON cm.concept_id = c.id
            GROUP BY c.id
            ORDER BY c.category, mentions DESC, c.name
        """).fetchall()

    conn.close()

    print(f"\n=== Concepts{f' ({category_filter})' if category_filter else ''} ===\n")
    current_cat = None
    for name, cat, mentions in rows:
        if cat != current_cat:
            current_cat = cat
            print(f"  [{cat}]")
        print(f"    {name} ({mentions} mentions)")


def show_stats():
    """Show database statistics."""
    conn = get_conn()

    print("\n=== Knowledge Base Statistics ===\n")

    for table, label in [
        ("documents", "Documents"),
        ("sections", "Sections"),
        ("concepts", "Concepts"),
        ("concept_mentions", "Concept Mentions"),
        ("decisions", "Decisions"),
        ("findings", "Findings"),
        ("risks", "Risks"),
        ("tags", "Tags"),
        ("cross_references", "Cross-References"),
    ]:
        count = conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
        print(f"  {label}: {count}")

    total_words = conn.execute("SELECT SUM(word_count) FROM documents").fetchone()[0]
    print(f"  Total Words: {total_words:,}")

    # Category breakdown
    print("\n  By Category:")
    for cat, count in conn.execute(
        "SELECT category, COUNT(*) FROM documents GROUP BY category ORDER BY COUNT(*) DESC"
    ).fetchall():
        print(f"    {cat}: {count}")

    # Top tags
    print("\n  Top 10 Tags:")
    for tag, count in conn.execute("""
        SELECT t.name, COUNT(dt.document_id)
        FROM tags t
        JOIN document_tags dt ON dt.tag_id = t.id
        GROUP BY t.id
        ORDER BY COUNT(dt.document_id) DESC
        LIMIT 10
    """).fetchall():
        print(f"    {tag}: {count}")

    conn.close()

    db_size = DB_PATH.stat().st_size / 1024
    print(f"\n  Database Size: {db_size:.1f} KB")


def docs_by_phase(phase: str):
    """List documents for a specific phase."""
    conn = get_conn()
    rows = conn.execute("""
        SELECT d.category, d.title, d.path, d.status, d.word_count
        FROM documents d
        WHERE d.phase LIKE ? OR d.phase = 'all'
        ORDER BY d.category, d.title
    """, (f"%{phase}%",)).fetchall()
    conn.close()

    print(f"\n=== Documents for Phase {phase} ===\n")
    for cat, title, path, status, words in rows:
        print(f"  [{cat}] {title} ({words} words)")
        print(f"    {path} [{status}]")
        print()


def docs_by_tag(tag: str):
    """List documents with a specific tag."""
    conn = get_conn()
    rows = conn.execute("""
        SELECT d.category, d.title, d.path, d.status
        FROM documents d
        JOIN document_tags dt ON dt.document_id = d.id
        JOIN tags t ON t.id = dt.tag_id
        WHERE t.name = ?
        ORDER BY d.category, d.title
    """, (tag,)).fetchall()
    conn.close()

    print(f"\n=== Documents tagged '{tag}' ===\n")
    for cat, title, path, status in rows:
        print(f"  [{cat}] {title} [{status}]")
        print(f"    {path}")
        print()


def find_doc(name: str):
    """Find a specific document by partial name."""
    conn = get_conn()
    rows = conn.execute("""
        SELECT d.category, d.title, d.path, d.status, d.phase, d.word_count, d.line_count
        FROM documents d
        WHERE d.path LIKE ? OR d.title LIKE ?
        ORDER BY d.title
    """, (f"%{name}%", f"%{name}%")).fetchall()
    conn.close()

    print(f"\n=== Documents matching '{name}' ===\n")
    for cat, title, path, status, phase, words, lines in rows:
        print(f"  [{cat}] {title}")
        print(f"    Path: {path}")
        print(f"    Status: {status} | Phase: {phase} | {words} words, {lines} lines")
        print()


def find_xrefs(target: str):
    """Find cross-references mentioning a target."""
    conn = get_conn()
    rows = conn.execute("""
        SELECT d.title, d.path, cr.link_text, cr.context
        FROM cross_references cr
        JOIN documents d ON d.id = cr.source_doc_id
        WHERE cr.target_path LIKE ?
        ORDER BY d.title
    """, (f"%{target}%",)).fetchall()
    conn.close()

    print(f"\n=== Cross-references to '{target}' ===\n")
    for title, path, link_text, context in rows:
        print(f"  [{title}]")
        print(f"    From: {path}")
        print(f"    Link: [{link_text}]")
        if context:
            print(f"    Context: {context[:100]}")
        print()


def find_related(name: str):
    """Find documents related to a given document via shared concepts and cross-refs."""
    conn = get_conn()

    # Find the source document
    doc = conn.execute(
        "SELECT id, title, path FROM documents WHERE path LIKE ? OR title LIKE ? LIMIT 1",
        (f"%{name}%", f"%{name}%"),
    ).fetchone()

    if not doc:
        print(f"\n  No document found matching '{name}'")
        conn.close()
        return

    doc_id, doc_title, doc_path = doc
    print(f"\n=== Related to: {doc_title} ===")
    print(f"    {doc_path}\n")

    # Find docs that share concepts (weighted by shared concept count)
    shared = conn.execute("""
        SELECT d2.title, d2.path, d2.category, COUNT(DISTINCT cm2.concept_id) AS shared_concepts
        FROM concept_mentions cm1
        JOIN concept_mentions cm2 ON cm2.concept_id = cm1.concept_id AND cm2.document_id != cm1.document_id
        JOIN documents d2 ON d2.id = cm2.document_id
        WHERE cm1.document_id = ? AND cm1.section_id IS NULL
        GROUP BY d2.id
        ORDER BY shared_concepts DESC
        LIMIT 10
    """, (doc_id,)).fetchall()

    if shared:
        print("  By Shared Concepts:")
        for title, path, cat, count in shared:
            print(f"    [{cat}] {title} ({count} shared concepts)")
            print(f"      {path}")

    # Find docs linked from this doc
    outgoing = conn.execute("""
        SELECT d2.title, d2.path, cr.link_text
        FROM cross_references cr
        JOIN documents d2 ON d2.id = cr.target_doc_id
        WHERE cr.source_doc_id = ?
    """, (doc_id,)).fetchall()

    if outgoing:
        print(f"\n  Links From This Doc ({len(outgoing)}):")
        for title, path, link_text in outgoing:
            print(f"    -> {title}")

    # Find docs linking to this doc
    incoming = conn.execute("""
        SELECT d2.title, d2.path, cr.link_text
        FROM cross_references cr
        JOIN documents d2 ON d2.id = cr.source_doc_id
        WHERE cr.target_doc_id = ?
    """, (doc_id,)).fetchall()

    if incoming:
        print(f"\n  Links To This Doc ({len(incoming)}):")
        for title, path, link_text in incoming:
            print(f"    <- {title}")

    conn.close()
    print()


def show_summary():
    """Quick project overview from the knowledge base."""
    conn = get_conn()

    total_docs = conn.execute("SELECT COUNT(*) FROM documents").fetchone()[0]
    total_words = conn.execute("SELECT SUM(word_count) FROM documents").fetchone()[0]

    print("\n=== Impulse Project Overview ===\n")
    print(f"  Knowledge Base: {total_docs} documents, {total_words:,} words\n")

    # Active decisions
    decisions = conn.execute("SELECT * FROM v_active_decisions").fetchall()
    print(f"  Active ADRs ({len(decisions)}):")
    for adr, title, status, date, path in decisions:
        print(f"    ADR-{adr:04d}: {title}")

    # Open risks by severity
    risks = conn.execute("SELECT severity, COUNT(*) FROM risks WHERE status IN ('open','mitigated') GROUP BY severity ORDER BY CASE severity WHEN 'CRITICAL' THEN 1 WHEN 'HIGH' THEN 2 WHEN 'MEDIUM' THEN 3 WHEN 'LOW' THEN 4 END").fetchall()
    print(f"\n  Open Risks:")
    for sev, count in risks:
        print(f"    {sev}: {count}")

    # Top concept categories
    print(f"\n  Concept Categories:")
    for cat, count in conn.execute("SELECT category, COUNT(*) FROM concepts GROUP BY category ORDER BY COUNT(*) DESC").fetchall():
        print(f"    {cat}: {count} concepts")

    # Findings by session
    findings = conn.execute("SELECT session, COUNT(*) FROM findings GROUP BY session ORDER BY session").fetchall()
    if findings:
        print(f"\n  Findings by Session:")
        for session, count in findings:
            print(f"    {session}: {count} findings")

    # Spec docs
    specs = conn.execute("SELECT title, path, word_count FROM documents WHERE category = 'spec' ORDER BY word_count DESC").fetchall()
    if specs:
        print(f"\n  Spec Documents:")
        for title, path, wc in specs:
            print(f"    {title} ({wc:,} words) — {path}")

    conn.close()
    print()


# ============================================================================
# Contribution Commands
# ============================================================================

def add_finding(severity: str, title: str, description: str = "", session: str = "agent"):
    """Add a finding to the knowledge base."""
    severity = severity.upper()
    if severity not in ("CRITICAL", "HIGH", "MEDIUM", "LOW"):
        print(f"  Invalid severity: {severity}. Use CRITICAL, HIGH, MEDIUM, or LOW.", file=sys.stderr)
        sys.exit(1)

    conn = get_conn()
    conn.execute(
        "INSERT INTO findings (session, severity, title, description, status) VALUES (?, ?, ?, ?, 'open')",
        (session, severity, title, description or title),
    )
    # Update FTS index
    row_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
    conn.execute(
        "INSERT INTO findings_fts(rowid, title, description, recommendation) VALUES (?, ?, ?, '')",
        (row_id, title, description or title),
    )
    conn.commit()
    conn.close()
    print(f"  Added [{severity}] finding: {title}")


def add_risk(severity: str, description: str, mitigation: str = ""):
    """Add a risk to the knowledge base."""
    severity = severity.upper()
    if severity not in ("CRITICAL", "HIGH", "MEDIUM", "LOW"):
        print(f"  Invalid severity: {severity}. Use CRITICAL, HIGH, MEDIUM, or LOW.", file=sys.stderr)
        sys.exit(1)

    conn = get_conn()
    conn.execute(
        "INSERT INTO risks (description, severity, status, mitigation) VALUES (?, ?, 'open', ?)",
        (description, severity, mitigation),
    )
    conn.commit()
    conn.close()
    print(f"  Added [{severity}] risk: {description[:80]}")


def add_concept(name: str, category: str, description: str = ""):
    """Add a concept to the knowledge base."""
    valid_categories = ("technology", "pattern", "principle", "hook", "file", "agent", "constraint", "tool")
    if category not in valid_categories:
        print(f"  Invalid category: {category}. Use one of: {', '.join(valid_categories)}", file=sys.stderr)
        sys.exit(1)

    conn = get_conn()
    try:
        conn.execute(
            "INSERT INTO concepts (name, category, description) VALUES (?, ?, ?)",
            (name, category, description),
        )
        # Update FTS index
        row_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
        conn.execute(
            "INSERT INTO concepts_fts(rowid, name, description, category) VALUES (?, ?, ?, ?)",
            (row_id, name, description, category),
        )
        conn.commit()
        print(f"  Added [{category}] concept: {name}")
    except sqlite3.IntegrityError:
        print(f"  Concept '{name}' already exists. Skipping.")
    conn.close()


def log_session(session_name: str, summary: str):
    """Log a session summary as a finding for traceability."""
    conn = get_conn()
    conn.execute(
        "INSERT INTO findings (session, severity, title, description, status) VALUES (?, 'LOW', ?, ?, 'addressed')",
        (session_name, f"Session log: {session_name}", summary),
    )
    row_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
    conn.execute(
        "INSERT INTO findings_fts(rowid, title, description, recommendation) VALUES (?, ?, ?, '')",
        (row_id, f"Session log: {session_name}", summary),
    )
    conn.commit()
    conn.close()
    print(f"  Logged session: {session_name}")


# ============================================================================
# Maintenance Commands
# ============================================================================

def check_staleness():
    """Check if the knowledge DB is stale (docs changed since last build)."""
    import os
    from datetime import datetime as dt

    if not DB_PATH.exists():
        print("  STALE: Knowledge database does not exist. Run: kdb --rebuild")
        sys.exit(1)

    db_mtime = os.path.getmtime(str(DB_PATH))
    db_time = dt.fromtimestamp(db_mtime)

    project_root = Path(__file__).parent.parent
    docs_dir = project_root / "docs"
    claude_md = project_root / "CLAUDE.md"

    stale_files = []

    # Check all markdown files
    for md_file in docs_dir.rglob("*.md"):
        if os.path.getmtime(str(md_file)) > db_mtime:
            stale_files.append(str(md_file.relative_to(project_root)))

    # Check CLAUDE.md
    if claude_md.exists() and os.path.getmtime(str(claude_md)) > db_mtime:
        stale_files.append("CLAUDE.md")

    if stale_files:
        print(f"\n  STALE: {len(stale_files)} file(s) changed since last build ({db_time.strftime('%Y-%m-%d %H:%M')})\n")
        for f in stale_files[:10]:
            print(f"    {f}")
        if len(stale_files) > 10:
            print(f"    ... and {len(stale_files) - 10} more")
        print(f"\n  Run: kdb --rebuild")
        sys.exit(1)
    else:
        print(f"  CURRENT: Knowledge DB is up to date (built {db_time.strftime('%Y-%m-%d %H:%M')})")
        sys.exit(0)


# ============================================================================
# CLI Entry Point
# ============================================================================

if __name__ == "__main__":
    args = sys.argv[1:]

    if not args:
        print(__doc__)
        sys.exit(0)

    if args[0] == "--decisions":
        list_decisions(include_superseded=("--all" in args))
    elif args[0] == "--all-decisions":
        list_decisions(include_superseded=True)
    elif args[0] == "--risks":
        list_risks()
    elif args[0] == "--concepts":
        cat = args[1] if len(args) > 1 else None
        list_concepts(cat)
    elif args[0] == "--stats":
        show_stats()
    elif args[0] == "--summary":
        show_summary()
    elif args[0] == "--phase":
        docs_by_phase(args[1] if len(args) > 1 else "1")
    elif args[0] == "--tag":
        docs_by_tag(args[1] if len(args) > 1 else "hooks")
    elif args[0] == "--doc":
        find_doc(args[1] if len(args) > 1 else "")
    elif args[0] == "--related":
        find_related(args[1] if len(args) > 1 else "")
    elif args[0] == "--xref":
        find_xrefs(args[1] if len(args) > 1 else "")
    elif args[0] == "--json":
        search(" ".join(args[1:]), as_json=True)
    # Contribution commands
    elif args[0] == "--add-finding":
        if len(args) < 3:
            print("Usage: kdb --add-finding SEVERITY \"title\" [\"description\"]")
            sys.exit(1)
        add_finding(args[1], args[2], args[3] if len(args) > 3 else "")
    elif args[0] == "--add-risk":
        if len(args) < 3:
            print("Usage: kdb --add-risk SEVERITY \"description\" [\"mitigation\"]")
            sys.exit(1)
        add_risk(args[1], args[2], args[3] if len(args) > 3 else "")
    elif args[0] == "--add-concept":
        if len(args) < 3:
            print("Usage: kdb --add-concept \"name\" \"category\" [\"description\"]")
            sys.exit(1)
        add_concept(args[1], args[2], args[3] if len(args) > 3 else "")
    elif args[0] == "--log":
        if len(args) < 3:
            print("Usage: kdb --log \"session-name\" \"summary\"")
            sys.exit(1)
        log_session(args[1], args[2])
    # Maintenance commands
    elif args[0] == "--check":
        check_staleness()
    else:
        search(" ".join(args))
