#!/usr/bin/env python3
"""
KDB Status — One-line health summary for the knowledge database.

Usage:
    python3 status.py              # Human-readable one-liner
    python3 status.py --porcelain  # Machine-parseable (space-separated, no labels)
    python3 status.py --json       # Full stats as JSON

Exit codes:
    0 = CURRENT
    1 = STALE
    2 = MISSING
"""

from __future__ import annotations

import json
import sys

from kdb_common import DB_PATH, check_staleness_data, get_db_stats


EXIT_CODES = {"CURRENT": 0, "STALE": 1, "MISSING": 2}


def main() -> None:
    porcelain = "--porcelain" in sys.argv
    json_out = "--json" in sys.argv

    staleness = check_staleness_data()
    status = staleness["status"]
    exit_code = EXIT_CODES.get(status, 2)

    if status == "MISSING":
        if json_out:
            print(json.dumps({"status": "MISSING", "error": staleness["message"]}))
        elif porcelain:
            print("MISSING 0 0 0 0")
        else:
            print(f"KDB: MISSING | {staleness['message']}")
        sys.exit(exit_code)

    try:
        stats = get_db_stats()
    except Exception as e:
        if json_out:
            print(json.dumps({"status": "ERROR", "error": str(e)}))
        elif porcelain:
            print(f"ERROR 0 0 0 0")
        else:
            print(f"KDB: ERROR | {e}")
        sys.exit(2)

    docs = stats.get("documents", 0)
    findings = stats.get("findings", 0)
    risks = stats.get("risks", 0)
    db_size_kb = stats.get("db_size_kb", 0)
    db_size_mb = round(db_size_kb / 1024, 1)

    if json_out:
        output = {
            "status": status,
            "built_at": staleness.get("built_at"),
            **stats,
        }
        if status == "STALE":
            output["stale_files"] = staleness.get("stale_files", [])
        print(json.dumps(output, indent=2))
    elif porcelain:
        print(f"{status} {docs} {findings} {risks} {db_size_kb}")
    else:
        size_label = f"{db_size_mb}MB" if db_size_mb >= 1.0 else f"{db_size_kb}KB"
        print(f"KDB: {status} | {docs} docs | {findings} findings | {risks} risks | {size_label}")

    sys.exit(exit_code)


if __name__ == "__main__":
    main()
