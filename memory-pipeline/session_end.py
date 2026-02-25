#!/usr/bin/env python3
"""
KDB Session End — Auto-wrap-up for coding agent sessions.

Replaces 3 manual kdb commands (--log, --rebuild, --check) with 1.

Usage:
    kdb-session-end --session "s7" --summary "Built automation tools"
    kdb-session-end --session "s7" --summary "Built tools" --rebuild
    kdb-session-end --session "s7" --summary "Built tools" --json
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

from kdb_common import (
    check_staleness_data,
    ensure_contributions_table,
    get_conn,
    get_db_stats,
)

SCRIPT_DIR = Path(__file__).parent


def log_session(conn, session_name: str, summary: str) -> int:
    """Log a session summary as a finding. Returns the row id."""
    conn.execute(
        "INSERT INTO findings (session, severity, title, description, status) "
        "VALUES (?, 'LOW', ?, ?, 'addressed')",
        (session_name, f"Session log: {session_name}", summary),
    )
    # FTS index updated automatically by findings_ai trigger
    row_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]

    # Track in session_contributions
    ensure_contributions_table(conn)
    conn.execute(
        "INSERT INTO session_contributions (session, table_name, row_id) "
        "VALUES (?, 'findings', ?)",
        (session_name, row_id),
    )

    conn.commit()
    return row_id


def run_rebuild() -> tuple[bool, str]:
    """Run extract.py to rebuild the database. Returns (success, output)."""
    try:
        result = subprocess.run(
            [sys.executable, str(SCRIPT_DIR / "extract.py")],
            capture_output=True,
            text=True,
            timeout=30,
        )
        output = result.stdout + result.stderr
        return result.returncode == 0, output.strip()
    except subprocess.TimeoutExpired:
        return False, "Rebuild timed out after 30 seconds"
    except Exception as e:
        return False, f"Rebuild failed: {e}"


def session_end(
    session_name: str,
    summary: str,
    rebuild: bool = False,
    as_json: bool = False,
) -> None:
    """Run the full session end sequence."""
    result: dict = {
        "session": session_name,
        "logged": False,
        "rebuild": {"requested": rebuild, "ran": False, "success": False},
        "staleness": {},
        "stats": {},
    }

    # 1. Log the session
    try:
        conn = get_conn()
        try:
            log_session(conn, session_name, summary)
            result["logged"] = True
        finally:
            conn.close()
    except Exception as e:
        result["log_error"] = str(e)

    # 2. Check staleness
    staleness = check_staleness_data()
    result["staleness"] = staleness

    # 3. Rebuild if requested or stale
    should_rebuild = rebuild or staleness["status"] == "STALE"
    if should_rebuild:
        result["rebuild"]["ran"] = True
        success, output = run_rebuild()
        result["rebuild"]["success"] = success
        result["rebuild"]["output"] = output

        # Re-check staleness after rebuild
        if success:
            result["staleness"] = check_staleness_data()

    # 4. Final stats
    try:
        result["stats"] = get_db_stats()
    except Exception:
        pass

    if as_json:
        print(json.dumps(result, indent=2))
    else:
        _print_human(result)


def _print_human(result: dict) -> None:
    """Print human-readable session end output."""
    print(f"\n=== KDB Session End ===\n")

    # Log status
    if result["logged"]:
        print(f"  [OK] Logged session: {result['session']}")
    else:
        print(f"  [!!] Failed to log session: {result.get('log_error', 'unknown')}")

    # Staleness
    staleness = result["staleness"]
    if staleness.get("status") == "STALE" and not result["rebuild"]["ran"]:
        print(f"  [!!] {staleness['message']}")
        print(f"       Run: kdb --rebuild  (or pass --rebuild)")
    elif staleness.get("status") == "CURRENT":
        print(f"  [OK] {staleness['message']}")

    # Rebuild
    rebuild = result["rebuild"]
    if rebuild["ran"]:
        if rebuild["success"]:
            print(f"  [OK] Database rebuilt successfully")
        else:
            print(f"  [!!] Rebuild failed: {rebuild.get('output', '')[:200]}")

    # Stats
    stats = result.get("stats", {})
    if stats:
        print(f"\n  DB: {stats.get('documents', 0)} docs, "
              f"{stats.get('findings', 0)} findings, "
              f"{stats.get('risks', 0)} risks, "
              f"{stats.get('total_words', 0):,} words")

    print()


if __name__ == "__main__":
    args = sys.argv[1:]
    session_name = None
    summary = None
    rebuild = False
    as_json = False

    i = 0
    while i < len(args):
        if args[i] == "--session" and i + 1 < len(args):
            session_name = args[i + 1]
            i += 2
        elif args[i] == "--summary" and i + 1 < len(args):
            summary = args[i + 1]
            i += 2
        elif args[i] == "--rebuild":
            rebuild = True
            i += 1
        elif args[i] == "--json":
            as_json = True
            i += 1
        else:
            i += 1

    if not session_name or not summary:
        print(
            "Usage: kdb-session-end --session <name> --summary <text> [--rebuild] [--json]",
            file=sys.stderr,
        )
        sys.exit(1)

    session_end(
        session_name=session_name,
        summary=summary,
        rebuild=rebuild,
        as_json=as_json,
    )
