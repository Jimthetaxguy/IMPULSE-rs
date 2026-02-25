"""
Shared utilities for KDB automation tools.

Provides importable functions that return data (dicts, connections)
instead of printing to stdout or calling sys.exit() like query.py does.

Used by: session_start.py, session_end.py, bulk_contribute.py
"""

from __future__ import annotations

import os
import sqlite3
from datetime import datetime
from pathlib import Path
from typing import Any

DB_PATH = Path(__file__).parent / "knowledge.db"
PROJECT_ROOT = Path(__file__).parent.parent


def get_conn() -> sqlite3.Connection:
    """Get a connection to the knowledge database.

    Returns a connection or raises FileNotFoundError if the DB doesn't exist.
    Unlike query.py's get_conn(), this never calls sys.exit().
    """
    if not DB_PATH.exists():
        raise FileNotFoundError(
            f"Knowledge database not found at {DB_PATH}. Run: kdb --rebuild"
        )
    return sqlite3.connect(str(DB_PATH))


def escape_fts5(term: str) -> str:
    """Escape a search term for safe FTS5 MATCH queries.

    FTS5 interprets certain characters as operators:
    - Hyphens as NOT
    - AND/OR/NOT as boolean operators
    - Colons as column filters

    We strip any existing quotes and wrap each word in double quotes
    to treat them as literal strings.
    """
    cleaned = term.replace('"', "")
    words = cleaned.split()
    if not words:
        return '""'
    return " ".join(f'"{w}"' for w in words)


def check_staleness_data() -> dict[str, Any]:
    """Check if the knowledge DB is stale (docs changed since last build).

    Returns:
        {
            "status": "CURRENT" | "STALE" | "MISSING",
            "stale_files": [...],
            "built_at": "2026-02-21 14:30" | None,
            "message": "human-readable summary"
        }
    """
    if not DB_PATH.exists():
        return {
            "status": "MISSING",
            "stale_files": [],
            "built_at": None,
            "message": "Knowledge database does not exist. Run: kdb --rebuild",
        }

    db_mtime = os.path.getmtime(str(DB_PATH))
    db_time = datetime.fromtimestamp(db_mtime)
    built_at = db_time.strftime("%Y-%m-%d %H:%M")

    docs_dir = PROJECT_ROOT / "docs"
    claude_md = PROJECT_ROOT / "CLAUDE.md"

    stale_files: list[str] = []

    # Check all markdown files in docs/
    if docs_dir.exists():
        for md_file in docs_dir.rglob("*.md"):
            if os.path.getmtime(str(md_file)) > db_mtime:
                stale_files.append(str(md_file.relative_to(PROJECT_ROOT)))

    # Check CLAUDE.md
    if claude_md.exists() and os.path.getmtime(str(claude_md)) > db_mtime:
        stale_files.append("CLAUDE.md")

    if stale_files:
        return {
            "status": "STALE",
            "stale_files": stale_files,
            "built_at": built_at,
            "message": f"{len(stale_files)} file(s) changed since last build ({built_at})",
        }

    return {
        "status": "CURRENT",
        "stale_files": [],
        "built_at": built_at,
        "message": f"Knowledge DB is up to date (built {built_at})",
    }


def get_db_stats(conn: sqlite3.Connection | None = None) -> dict[str, Any]:
    """Get database statistics as a dict.

    Args:
        conn: Optional existing connection. If None, opens and closes its own.

    Returns:
        {
            "documents": int,
            "sections": int,
            "concepts": int,
            "decisions": int,
            "findings": int,
            "risks": int,
            "tags": int,
            "cross_references": int,
            "total_words": int,
            "db_size_kb": float
        }
    """
    close_after = conn is None
    if conn is None:
        conn = get_conn()

    stats: dict[str, Any] = {}

    for table in [
        "documents",
        "sections",
        "concepts",
        "decisions",
        "findings",
        "risks",
        "tags",
        "cross_references",
    ]:
        stats[table] = conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]

    total_words = conn.execute("SELECT SUM(word_count) FROM documents").fetchone()[0]
    stats["total_words"] = total_words or 0

    if close_after:
        conn.close()

    stats["db_size_kb"] = round(DB_PATH.stat().st_size / 1024, 1)

    return stats


def ensure_contributions_table(conn: sqlite3.Connection) -> None:
    """Create the session_contributions tracking table if it doesn't exist.

    This table tracks which items were contributed by automation tools,
    enabling session-level auditing without requiring a full DB rebuild.
    """
    conn.execute("""
        CREATE TABLE IF NOT EXISTS session_contributions (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            session         TEXT    NOT NULL,
            table_name      TEXT    NOT NULL,
            row_id          INTEGER NOT NULL,
            contributed_at  TEXT    DEFAULT (datetime('now'))
        )
    """)
    conn.execute("""
        CREATE INDEX IF NOT EXISTS idx_session_contributions_session
        ON session_contributions(session)
    """)
    conn.commit()
