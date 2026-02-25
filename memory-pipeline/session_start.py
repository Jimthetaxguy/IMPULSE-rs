#!/usr/bin/env python3
"""
KDB Session Start — Auto-bootstrap for coding agent sessions.

Replaces 4 manual kdb commands (--check, search, --decisions, --risks) with 1.

Usage:
    kdb-session-start                          # Human-readable output
    kdb-session-start --task "hooks impl"      # Include task-relevant search
    kdb-session-start --json                   # JSON output for agents
    kdb-session-start --task "hooks" --json    # Both
"""

from __future__ import annotations

import json
import sys

from kdb_common import (
    check_staleness_data,
    escape_fts5,
    get_conn,
    get_db_stats,
)


def get_active_decisions(conn) -> list[dict]:
    """Get active ADR decisions."""
    rows = conn.execute("""
        SELECT d.adr_number, d.title, d.status, d.date, doc.path
        FROM decisions d
        LEFT JOIN documents doc ON doc.id = d.document_id
        WHERE d.status = 'accepted'
        ORDER BY d.adr_number
    """).fetchall()
    return [
        {
            "adr_number": adr,
            "title": title,
            "status": status,
            "date": date,
            "path": path,
        }
        for adr, title, status, date, path in rows
    ]


def get_open_risks(conn) -> list[dict]:
    """Get open risks ordered by severity."""
    rows = conn.execute("""
        SELECT r.severity, r.description, r.mitigation, r.phase, d.path
        FROM risks r
        LEFT JOIN documents d ON d.id = r.document_id
        WHERE r.status IN ('open', 'mitigated')
        ORDER BY
            CASE r.severity
                WHEN 'CRITICAL' THEN 1
                WHEN 'HIGH' THEN 2
                WHEN 'MEDIUM' THEN 3
                WHEN 'LOW' THEN 4
            END
    """).fetchall()
    return [
        {
            "severity": sev,
            "description": desc[:200],
            "mitigation": mit,
            "phase": phase,
            "path": path,
        }
        for sev, desc, mit, phase, path in rows
    ]


def search_for_task(conn, task: str) -> list[dict]:
    """Search documents, decisions, and findings for task-relevant results."""
    safe_term = escape_fts5(task)
    results = []

    # Search documents
    rows = conn.execute("""
        SELECT d.category, d.title, d.path,
               snippet(documents_fts, 1, '>>>', '<<<', '...', 30) AS snippet
        FROM documents_fts
        JOIN documents d ON d.id = documents_fts.rowid
        WHERE documents_fts MATCH ?
        ORDER BY rank
        LIMIT 5
    """, (safe_term,)).fetchall()

    for cat, title, path, snippet in rows:
        results.append({
            "type": "document",
            "category": cat,
            "title": title,
            "path": path,
            "snippet": snippet,
        })

    # Search decisions
    rows = conn.execute("""
        SELECT dec.title, dec.status, dec.adr_number,
               snippet(decisions_fts, 2, '>>>', '<<<', '...', 30) AS snippet
        FROM decisions_fts
        JOIN decisions dec ON dec.id = decisions_fts.rowid
        WHERE decisions_fts MATCH ?
        ORDER BY rank
        LIMIT 3
    """, (safe_term,)).fetchall()

    for title, status, adr, snippet in rows:
        results.append({
            "type": "decision",
            "title": f"ADR-{adr:04d}: {title}" if adr else title,
            "status": status,
            "snippet": snippet,
        })

    # Search findings
    rows = conn.execute("""
        SELECT f.severity, f.title, f.session,
               snippet(findings_fts, 1, '>>>', '<<<', '...', 30) AS snippet
        FROM findings_fts
        JOIN findings f ON f.id = findings_fts.rowid
        WHERE findings_fts MATCH ?
        ORDER BY rank
        LIMIT 3
    """, (safe_term,)).fetchall()

    for sev, title, session, snippet in rows:
        results.append({
            "type": "finding",
            "severity": sev,
            "title": title,
            "session": session,
            "snippet": snippet,
        })

    return results


def get_recent_sessions(conn, limit: int = 3) -> list[dict]:
    """Get recent session logs."""
    rows = conn.execute("""
        SELECT session, title, description, severity
        FROM findings
        WHERE title LIKE 'Session log:%'
        ORDER BY id DESC
        LIMIT ?
    """, (limit,)).fetchall()
    return [
        {
            "session": session,
            "title": title.replace("Session log: ", ""),
            "summary": desc[:200],
        }
        for session, title, desc, sev in rows
    ]


def session_start(task: str | None = None, as_json: bool = False) -> None:
    """Run the full session start sequence."""
    result: dict = {}

    # 1. Staleness check
    staleness = check_staleness_data()
    result["staleness"] = staleness

    # 2-5. Query the DB (if it exists)
    if staleness["status"] != "MISSING":
        conn = get_conn()
        result["decisions"] = get_active_decisions(conn)
        result["risks"] = get_open_risks(conn)
        result["recent_sessions"] = get_recent_sessions(conn)
        result["stats"] = get_db_stats(conn)

        if task:
            result["task_search"] = search_for_task(conn, task)

        conn.close()

    if as_json:
        print(json.dumps(result, indent=2))
    else:
        _print_human(result, task)


def _print_human(result: dict, task: str | None) -> None:
    """Print human-readable session start output."""
    staleness = result["staleness"]
    status_icon = {
        "CURRENT": "OK",
        "STALE": "!!",
        "MISSING": "XX",
    }.get(staleness["status"], "??")

    print(f"\n=== KDB Session Start ===\n")
    print(f"  [{status_icon}] {staleness['message']}")

    if staleness["status"] == "STALE":
        for f in staleness["stale_files"][:5]:
            print(f"      {f}")
        if len(staleness["stale_files"]) > 5:
            print(f"      ... and {len(staleness['stale_files']) - 5} more")
        print(f"      Run: kdb --rebuild")

    if staleness["status"] == "MISSING":
        print()
        return

    # Decisions
    decisions = result.get("decisions", [])
    print(f"\n  Active ADRs ({len(decisions)}):")
    for d in decisions:
        adr = d["adr_number"]
        print(f"    ADR-{adr:04d}: {d['title']}")

    # Risks
    risks = result.get("risks", [])
    if risks:
        print(f"\n  Open Risks ({len(risks)}):")
        for r in risks:
            desc_short = r["description"][:100].replace("\n", " ")
            print(f"    [{r['severity']}] {desc_short}")

    # Task search
    if task and result.get("task_search"):
        results = result["task_search"]
        print(f"\n  Prior Work on \"{task}\" ({len(results)} results):")
        for r in results:
            label = r.get("category", r["type"])
            print(f"    [{label}] {r['title']}")
            if r.get("snippet"):
                snippet_clean = r["snippet"].replace(">>>", "").replace("<<<", "")
                print(f"      {snippet_clean[:120]}")

    # Recent sessions
    sessions = result.get("recent_sessions", [])
    if sessions:
        print(f"\n  Recent Sessions:")
        for s in sessions:
            print(f"    {s['title']}: {s['summary'][:80]}")

    # Stats summary
    stats = result.get("stats", {})
    if stats:
        print(f"\n  DB: {stats.get('documents', 0)} docs, "
              f"{stats.get('total_words', 0):,} words, "
              f"{stats.get('db_size_kb', 0)} KB")

    print()


if __name__ == "__main__":
    args = sys.argv[1:]
    task = None
    as_json = False

    i = 0
    while i < len(args):
        if args[i] == "--task" and i + 1 < len(args):
            task = args[i + 1]
            i += 2
        elif args[i] == "--json":
            as_json = True
            i += 1
        else:
            # Treat positional args as task description
            task = " ".join(args[i:])
            break

    session_start(task=task, as_json=as_json)
