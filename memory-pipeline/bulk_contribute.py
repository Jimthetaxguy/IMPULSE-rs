#!/usr/bin/env python3
"""
KDB Bulk Contribute — JSON-in, JSON-out bulk contribution tool.

Replaces multiple kdb --add-* calls with a single JSON pipe.

Usage:
    echo '{"findings":[{"severity":"HIGH","title":"Bug found"}]}' | kdb-contribute
    kdb-contribute --inline '{"risks":[{"severity":"LOW","description":"Minor issue"}]}'

Input schema:
    {
        "session": "optional-session-name",
        "findings": [{"severity": "HIGH", "title": "...", "description": "..."}],
        "risks": [{"severity": "MEDIUM", "description": "...", "mitigation": "..."}],
        "concepts": [{"name": "...", "category": "technology", "description": "..."}]
    }

Output:
    {"inserted": {"findings": 2, "risks": 1}, "skipped": {"concepts": 1}, "errors": []}
"""

from __future__ import annotations

import json
import sqlite3
import sys

from kdb_common import ensure_contributions_table, get_conn

VALID_SEVERITIES = {"CRITICAL", "HIGH", "MEDIUM", "LOW"}
VALID_CATEGORIES = {
    "technology", "pattern", "principle", "hook",
    "file", "agent", "constraint", "tool",
}


def validate_finding(f: dict, index: int) -> str | None:
    """Validate a finding dict. Returns error string or None."""
    if "title" not in f:
        return f"findings[{index}]: missing 'title'"
    sev = f.get("severity", "").upper()
    if sev not in VALID_SEVERITIES:
        return f"findings[{index}]: invalid severity '{f.get('severity')}' (use {', '.join(sorted(VALID_SEVERITIES))})"
    return None


def validate_risk(r: dict, index: int) -> str | None:
    """Validate a risk dict. Returns error string or None."""
    if "description" not in r:
        return f"risks[{index}]: missing 'description'"
    sev = r.get("severity", "").upper()
    if sev not in VALID_SEVERITIES:
        return f"risks[{index}]: invalid severity '{r.get('severity')}' (use {', '.join(sorted(VALID_SEVERITIES))})"
    return None


def validate_concept(c: dict, index: int) -> str | None:
    """Validate a concept dict. Returns error string or None."""
    if "name" not in c:
        return f"concepts[{index}]: missing 'name'"
    if "category" not in c:
        return f"concepts[{index}]: missing 'category'"
    if c["category"] not in VALID_CATEGORIES:
        return f"concepts[{index}]: invalid category '{c['category']}' (use {', '.join(sorted(VALID_CATEGORIES))})"
    return None


def bulk_contribute(data: dict) -> dict:
    """Process a bulk contribution payload.

    Returns a result dict with inserted counts, skipped counts, and errors.
    All valid inserts happen in a single transaction.
    """
    session = data.get("session", "agent")
    findings = data.get("findings", [])
    risks = data.get("risks", [])
    concepts = data.get("concepts", [])

    result = {
        "inserted": {"findings": 0, "risks": 0, "concepts": 0},
        "skipped": {"concepts": 0},
        "errors": [],
    }

    # Validate all items first (fail fast on validation errors)
    valid_findings = []
    for i, f in enumerate(findings):
        err = validate_finding(f, i)
        if err:
            result["errors"].append(err)
        else:
            valid_findings.append(f)

    valid_risks = []
    for i, r in enumerate(risks):
        err = validate_risk(r, i)
        if err:
            result["errors"].append(err)
        else:
            valid_risks.append(r)

    valid_concepts = []
    for i, c in enumerate(concepts):
        err = validate_concept(c, i)
        if err:
            result["errors"].append(err)
        else:
            valid_concepts.append(c)

    # Nothing valid to insert
    if not valid_findings and not valid_risks and not valid_concepts:
        return result

    conn = get_conn()
    ensure_contributions_table(conn)

    # Track counts separately; only report after successful commit
    pending = {"findings": 0, "risks": 0, "concepts": 0}
    skipped_concepts = 0

    try:
        # Insert findings (FTS index updated by findings_ai trigger)
        for f in valid_findings:
            sev = f["severity"].upper()
            title = f["title"]
            desc = f.get("description", title)
            conn.execute(
                "INSERT INTO findings (session, severity, title, description, status) "
                "VALUES (?, ?, ?, ?, 'open')",
                (session, sev, title, desc),
            )
            row_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
            conn.execute(
                "INSERT INTO session_contributions (session, table_name, row_id) "
                "VALUES (?, 'findings', ?)",
                (session, row_id),
            )
            pending["findings"] += 1

        # Insert risks (no FTS table for risks)
        for r in valid_risks:
            sev = r["severity"].upper()
            desc = r["description"]
            mit = r.get("mitigation", "")
            conn.execute(
                "INSERT INTO risks (description, severity, status, mitigation) "
                "VALUES (?, ?, 'open', ?)",
                (desc, sev, mit),
            )
            row_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
            conn.execute(
                "INSERT INTO session_contributions (session, table_name, row_id) "
                "VALUES (?, 'risks', ?)",
                (session, row_id),
            )
            pending["risks"] += 1

        # Insert concepts (skip duplicates; FTS index updated by concepts_ai trigger)
        for c in valid_concepts:
            name = c["name"]
            cat = c["category"]
            desc = c.get("description", "")
            try:
                conn.execute(
                    "INSERT INTO concepts (name, category, description) "
                    "VALUES (?, ?, ?)",
                    (name, cat, desc),
                )
                row_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
                conn.execute(
                    "INSERT INTO session_contributions (session, table_name, row_id) "
                    "VALUES (?, 'concepts', ?)",
                    (session, row_id),
                )
                pending["concepts"] += 1
            except sqlite3.IntegrityError:
                skipped_concepts += 1

        conn.commit()
        # Only report counts after successful commit
        result["inserted"]["findings"] = pending["findings"]
        result["inserted"]["risks"] = pending["risks"]
        result["inserted"]["concepts"] = pending["concepts"]
        result["skipped"]["concepts"] = skipped_concepts
    except Exception as e:
        conn.rollback()
        result["errors"].append(f"Transaction failed: {e}")
    finally:
        conn.close()

    return result


if __name__ == "__main__":
    args = sys.argv[1:]

    # Read input from --inline flag or stdin
    raw_input = None

    i = 0
    while i < len(args):
        if args[i] == "--inline" and i + 1 < len(args):
            raw_input = args[i + 1]
            i += 2
        else:
            i += 1

    if raw_input is None:
        if sys.stdin.isatty():
            print(
                "Usage: echo '{\"findings\":[...]}' | kdb-contribute\n"
                "       kdb-contribute --inline '{\"findings\":[...]}'",
                file=sys.stderr,
            )
            sys.exit(1)
        raw_input = sys.stdin.read()

    try:
        data = json.loads(raw_input)
    except json.JSONDecodeError as e:
        print(json.dumps({"inserted": {}, "skipped": {}, "errors": [f"Invalid JSON: {e}"]}))
        sys.exit(1)

    result = bulk_contribute(data)
    print(json.dumps(result, indent=2))

    # Exit with error code if there were errors
    if result["errors"]:
        sys.exit(1)
