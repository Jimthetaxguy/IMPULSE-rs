#!/usr/bin/env python3
"""Retrieval + injection performance harness for impulse-rs.

Runs repeated keyword/semantic/injection-review calls and validates p95 thresholds.
The harness is local-first and relies on existing `.impulse` fixture data.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Tuple


INJECTION_LINE_RE = re.compile(
    r"Injection:\s+requested=(?P<requested>\S+)\s+effective=(?P<effective>\S+)\s+"
    r"applied=(?P<applied>\S+)\s+backend=(?P<backend>\S+)\s+"
    r"fallback_code=(?P<fallback_code>\S+)\s+timing=(?P<timing_ms>\d+)ms\s+"
    r"candidates=(?P<candidates>\d+)\s+status=(?P<status>\S+)"
)


def percentile(values: List[float], p: float) -> float:
    if not values:
        return 0.0
    vals = sorted(values)
    idx = max(0, min(len(vals) - 1, int(round((p / 100.0) * (len(vals) - 1)))))
    return float(vals[idx])


def default_queries() -> List[str]:
    return [
        "session",
        "review",
        "memory",
        "fallback",
        "hooks",
        "retrieval",
        "context",
        "status",
        "genome",
        "history",
    ]


def load_queries(path: str | None) -> List[str]:
    if not path:
        return default_queries()

    p = Path(path)
    if not p.exists():
        raise SystemExit(f"Query file not found: {path}")

    data = json.loads(p.read_text(encoding="utf-8"))
    out: List[str] = []
    if isinstance(data, list):
        for item in data:
            if isinstance(item, str) and item.strip():
                out.append(item.strip())
            elif isinstance(item, dict):
                q = str(item.get("query", "")).strip()
                if q:
                    out.append(q)
    if not out:
        out = default_queries()
    return out


def run_cmd(cmd: List[str], env: Dict[str, str] | None = None) -> Tuple[int, str, str, float]:
    started = time.perf_counter()
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env)
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    return proc.returncode, proc.stdout, proc.stderr, elapsed_ms


def run_search(bin_path: str, impulse_dir: str, query: str, mode: str) -> Dict[str, Any]:
    code, stdout, stderr, wall_ms = run_cmd(
        [
            bin_path,
            "-c",
            impulse_dir,
            "search-history",
            "--query",
            query,
            "--mode",
            mode,
            "--json",
        ]
    )
    if code != 0:
        return {
            "ok": False,
            "query": query,
            "mode": mode,
            "wall_ms": wall_ms,
            "error": stderr.strip() or "command failed",
            "timing_ms": None,
            "used_fallback": None,
            "fallback_code": None,
        }

    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError as exc:
        return {
            "ok": False,
            "query": query,
            "mode": mode,
            "wall_ms": wall_ms,
            "error": f"invalid json output: {exc}",
            "timing_ms": None,
            "used_fallback": None,
            "fallback_code": None,
        }

    return {
        "ok": True,
        "query": query,
        "mode": mode,
        "wall_ms": wall_ms,
        "timing_ms": float(payload.get("timing_ms", 0.0)),
        "used_fallback": bool(payload.get("used_fallback", False)),
        "fallback_code": payload.get("fallback_code"),
        "backend_used": payload.get("backend_used"),
        "candidate_count": payload.get("candidate_count"),
    }


def run_injection_review(bin_path: str, impulse_dir: str, query: str) -> Dict[str, Any]:
    code, stdout, stderr, wall_ms = run_cmd(
        [
            bin_path,
            "-c",
            impulse_dir,
            "orchestrate",
            "--task",
            query,
            "--inject-mode",
            "review",
            "--inject-explain",
        ]
    )
    if code != 0:
        return {
            "ok": False,
            "query": query,
            "wall_ms": wall_ms,
            "error": stderr.strip() or "command failed",
        }

    line = ""
    for raw in stdout.splitlines():
        if raw.startswith("Injection:"):
            line = raw.strip()
            break

    parsed = {}
    if line:
        m = INJECTION_LINE_RE.search(line)
        if m:
            parsed = {
                "requested": m.group("requested"),
                "effective": m.group("effective"),
                "applied": m.group("applied") == "true",
                "backend": m.group("backend"),
                "fallback_code": m.group("fallback_code"),
                "timing_ms": float(m.group("timing_ms")),
                "candidates": int(m.group("candidates")),
                "status": m.group("status"),
            }

    return {
        "ok": True,
        "query": query,
        "wall_ms": wall_ms,
        "line": line,
        **parsed,
    }


def detect_vector_enabled(impulse_dir: str) -> bool:
    config_path = Path(impulse_dir) / "config.json"
    if not config_path.exists():
        return False
    try:
        config = json.loads(config_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return False
    return bool(config.get("retrieval_vector_enabled")) and config.get("retrieval_backend") == "fts+vec"


def resolve_bin(bin_arg: str | None, root: Path) -> str:
    if bin_arg:
        p = Path(bin_arg)
        if p.exists():
            return str(p)
    default = root / "impulse-rs" / "target" / "debug" / "impulse-rs"
    if default.exists():
        return str(default)
    which = shutil_which("impulse-rs")
    if which:
        return which
    raise SystemExit("Could not locate impulse-rs binary. Build with: cd impulse-rs && cargo build")


def shutil_which(name: str) -> str | None:
    for base in os.environ.get("PATH", "").split(os.pathsep):
        candidate = Path(base) / name
        if candidate.exists() and os.access(candidate, os.X_OK):
            return str(candidate)
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".", help="Repository root")
    parser.add_argument("--impulse-dir", default=".impulse", help="Path to impulse state directory")
    parser.add_argument("--bin", default="", help="Path to impulse-rs binary")
    parser.add_argument("--queries", default="", help="JSON file of query strings or objects with {query}")
    parser.add_argument("--iterations", type=int, default=30, help="Number of iterations")
    parser.add_argument("--out", default="docs/research/retrieval-perf-report.json")
    parser.add_argument("--keyword-p95-max", type=float, default=200.0)
    parser.add_argument("--semantic-p95-max", type=float, default=500.0)
    parser.add_argument("--injection-p95-max", type=float, default=700.0)
    parser.add_argument("--semantic-fallback-max", type=float, default=0.50)
    parser.add_argument("--injection-fallback-max", type=float, default=0.70)
    args = parser.parse_args()

    root = Path(args.root).resolve()
    impulse_dir = str((root / args.impulse_dir).resolve())
    if not Path(impulse_dir).exists():
        raise SystemExit(f"impulse dir not found: {impulse_dir}")

    bin_path = resolve_bin(args.bin or None, root)
    queries = load_queries(args.queries or None)
    iterations = max(1, args.iterations)

    keyword_runs: List[Dict[str, Any]] = []
    semantic_runs: List[Dict[str, Any]] = []
    injection_runs: List[Dict[str, Any]] = []

    for i in range(iterations):
        query = queries[i % len(queries)]
        keyword_runs.append(run_search(bin_path, impulse_dir, query, "keyword"))
        semantic_runs.append(run_search(bin_path, impulse_dir, query, "semantic"))
        injection_runs.append(run_injection_review(bin_path, impulse_dir, query))

    keyword_timings = [r["timing_ms"] for r in keyword_runs if r.get("ok") and r.get("timing_ms") is not None]
    semantic_timings = [r["timing_ms"] for r in semantic_runs if r.get("ok") and r.get("timing_ms") is not None]
    injection_wall = [r["wall_ms"] for r in injection_runs if r.get("ok")]

    semantic_fallback_rate = 0.0
    semantic_ok = [r for r in semantic_runs if r.get("ok")]
    if semantic_ok:
        semantic_fallback_rate = sum(1 for r in semantic_ok if r.get("used_fallback")) / float(len(semantic_ok))

    injection_fallback_rate = 0.0
    injection_ok = [r for r in injection_runs if r.get("ok")]
    if injection_ok:
        injection_fallback_rate = (
            sum(1 for r in injection_ok if str(r.get("fallback_code", "none")) != "none")
            / float(len(injection_ok))
        )

    keyword_p95 = percentile(keyword_timings, 95)
    semantic_p95 = percentile(semantic_timings, 95)
    injection_p95 = percentile(injection_wall, 95)

    vector_enabled = detect_vector_enabled(impulse_dir)

    checks = {
        "keyword_p95": {
            "actual": keyword_p95,
            "max": args.keyword_p95_max,
            "pass": keyword_p95 <= args.keyword_p95_max,
        },
        "semantic_p95": {
            "actual": semantic_p95,
            "max": args.semantic_p95_max,
            "pass": semantic_p95 <= args.semantic_p95_max,
        },
        "injection_review_p95": {
            "actual": injection_p95,
            "max": args.injection_p95_max,
            "pass": injection_p95 <= args.injection_p95_max,
        },
        "semantic_fallback_rate": {
            "actual": semantic_fallback_rate,
            "max": args.semantic_fallback_max,
            "pass": (semantic_fallback_rate <= args.semantic_fallback_max) if vector_enabled else True,
            "skipped": not vector_enabled,
        },
        "injection_fallback_rate": {
            "actual": injection_fallback_rate,
            "max": args.injection_fallback_max,
            "pass": (injection_fallback_rate <= args.injection_fallback_max) if vector_enabled else True,
            "skipped": not vector_enabled,
        },
    }

    overall_pass = all(item.get("pass", False) for item in checks.values())

    report = {
        "summary": {
            "iterations": iterations,
            "query_count": len(queries),
            "binary": bin_path,
            "impulse_dir": impulse_dir,
            "vector_backend_enabled": vector_enabled,
            "keyword_p95_ms": keyword_p95,
            "semantic_p95_ms": semantic_p95,
            "injection_review_p95_ms": injection_p95,
            "semantic_fallback_rate": semantic_fallback_rate,
            "injection_fallback_rate": injection_fallback_rate,
            "overall_pass": overall_pass,
        },
        "thresholds": checks,
        "samples": {
            "keyword": keyword_runs,
            "semantic": semantic_runs,
            "injection_review": injection_runs,
        },
        "notes": [
            "Semantic and injection fallback-rate assertions are skipped when vector backend is disabled.",
            "Injection p95 is measured as end-to-end orchestrate command wall-clock runtime in review mode.",
        ],
    }

    out_path = Path(args.out)
    if not out_path.is_absolute():
        out_path = root / out_path
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(report, indent=2), encoding="utf-8")

    print(f"Wrote perf report: {out_path}")
    print(
        "keyword_p95={:.2f}ms semantic_p95={:.2f}ms injection_review_p95={:.2f}ms".format(
            keyword_p95, semantic_p95, injection_p95
        )
    )
    print(
        "semantic_fallback_rate={:.2%} injection_fallback_rate={:.2%} vector_enabled={}".format(
            semantic_fallback_rate, injection_fallback_rate, vector_enabled
        )
    )

    return 0 if overall_pass else 1


if __name__ == "__main__":
    raise SystemExit(main())
