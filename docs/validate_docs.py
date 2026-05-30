#!/usr/bin/env python3
"""
Validate Impulse documentation metadata and contract consistency.

Usage:
    python3 docs/validate_docs.py                 # Front matter validation
    python3 docs/validate_docs.py --json          # JSON output
    python3 docs/validate_docs.py --contract      # Contract drift checks
    python3 docs/validate_docs.py --all           # Front matter + contract checks
    python3 docs/validate_docs.py --self-test     # Internal validator self-test
"""

import argparse
import datetime
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

ROOT_DIR = Path(__file__).resolve().parent.parent
DOCS_DIR = ROOT_DIR / "docs"

REQUIRED_FIELDS: List[str] = []
TYPES = [
    "agent_guidelines",
    "specification",
    "guide",
    "decision",
    "research",
    "vision",
    "metadata",
    "schema",
    "doc",
    "reference",
]
PHASES = ["phase0", "phase1", "phase1.5", "phase2", "phase3", "all", "historical"]
STATUSES = ["draft", "review", "active", "deprecated", "complete", "superseded", "archive", "accepted"]

CONTRACT_KEY_FILES = [
    ROOT_DIR / "AGENTS.md",
    ROOT_DIR / "CLAUDE.md",
    DOCS_DIR / "INDEX.md",
    DOCS_DIR / "SUMMARY.md",
]

CONTRACT_REQUIRED_MARKERS = {
    ROOT_DIR / "AGENTS.md": [
        "RUST-CANONICAL-CONTRACT.md",
        "COLLABORATIVE-AGENTIC-CODING.md",
        "Canonical stack: Rust (impulse-rs)",
        "Roadmap contract: Now=Rust core + Tauri desktop shell; Next=terminal bridge + daemon parity; Legacy=egui compile-maintenance only",
    ],
    ROOT_DIR / "CLAUDE.md": [
        "RUST-CANONICAL-CONTRACT.md",
        "COLLABORATIVE-AGENTIC-CODING.md",
        "Canonical stack: Rust (impulse-rs)",
        "Roadmap contract: Now=Rust core + Tauri desktop shell; Next=terminal bridge + daemon parity; Legacy=egui compile-maintenance only",
    ],
    DOCS_DIR / "INDEX.md": [
        "RUST-CANONICAL-CONTRACT.md",
        "COLLABORATIVE-AGENTIC-CODING.md",
        "Canonical stack: Rust (impulse-rs)",
        "Roadmap contract: Now=Rust core + Tauri desktop shell (Phase 0 docs reset), Next=egui boundary cleanup + static shell, Later=live terminal bridge + daemon parity",
    ],
    DOCS_DIR / "SUMMARY.md": [
        "RUST-CANONICAL-CONTRACT.md",
    ],
}

FORBIDDEN_ACTIVE_PHRASES = [
    "TypeScript/Bun ONLY. Zero Python. Zero Rust. Zero WASM.",
    "Target: TypeScript/Bun ONLY. Zero Python. Zero Rust. Zero databases.",
    "Roadmap contract: Now=Rust core + EGUI workbench, Next=daemon-truth EGUI + hook validation, Later=agent control + artifact polish",
    "Active EGUI Workbench Track",
    "Now: Rust Core + EGUI Workbench",
    "Next: Daemon-Truth EGUI + Hook Validation",
    "The active roadmap is now Rust core plus the EGUI operator workbench.",
    "EGUI/operator workbench is marked as active work",
    "active Rust-native EGUI surface",
]

NON_AUTHORITATIVE_STATUSES = {"superseded", "deprecated", "archive", "historical"}
DUPLICATE_ARTIFACT_PATTERN = re.compile(r"^(.+)\s2(\.[^.]+)$")


@dataclass
class ContractIssue:
    path: str
    line: int
    message: str


def extract_front_matter(content: str) -> Optional[Dict[str, Any]]:
    if not content.startswith("---"):
        return None
    parts = content.split("---", 2)
    if len(parts) < 3:
        return None
    try:
        return yaml.safe_load(parts[1])
    except yaml.YAMLError:
        return None


def validate_front_matter(front_matter: Optional[Dict[str, Any]]) -> List[str]:
    errors: List[str] = []
    if front_matter is None:
        return []

    for field in REQUIRED_FIELDS:
        if field not in front_matter:
            errors.append(f"Missing required field: {field}")

    if "type" in front_matter and front_matter["type"] not in TYPES:
        errors.append(f"Invalid type: {front_matter['type']}. Expected one of: {TYPES}")

    if "phase" in front_matter:
        phase = str(front_matter["phase"])
        valid = any(phase.startswith(p) for p in PHASES) or phase.replace("-", "").replace(".", "").isdigit()
        if not valid:
            errors.append(f"Invalid phase: {phase}. Expected one of: {PHASES}")

    if "status" in front_matter:
        status = str(front_matter["status"])
        if status not in STATUSES:
            errors.append(f"Invalid status: {status}. Expected one of: {STATUSES}")

    if "description" in front_matter:
        desc_len = len(str(front_matter["description"]))
        if desc_len < 10:
            errors.append(f"Description too short ({desc_len} chars, min 10)")
        elif desc_len > 200:
            errors.append(f"Description too long ({desc_len} chars, max 200)")

    return errors


def get_docs_files() -> List[Path]:
    docs = list(DOCS_DIR.rglob("*.md")) + list(DOCS_DIR.rglob("*.yaml"))
    excluded = {".DS_Store", "SUMMARY.yaml", "metadata.yaml"}
    return [p for p in docs if p.name not in excluded]


def detect_duplicate_artifacts(files: List[Path]) -> Dict[Path, List[str]]:
    errors: Dict[Path, List[str]] = {}
    for path in files:
        m = DUPLICATE_ARTIFACT_PATTERN.match(path.name)
        if not m:
            continue
        canonical_name = f"{m.group(1)}{m.group(2)}"
        msg = (
            f"Duplicate artifact filename at line 1: '{path.name}'. Remove this file and keep "
            f"'{canonical_name}' as the canonical document."
        )
        errors[path] = [msg]
    return errors


def find_line(content: str, needle: str) -> int:
    for idx, line in enumerate(content.splitlines(), start=1):
        if needle in line:
            return idx
    return 1


def check_contract_markers() -> List[ContractIssue]:
    issues: List[ContractIssue] = []
    for path in CONTRACT_KEY_FILES:
        if not path.exists():
            issues.append(ContractIssue(str(path.relative_to(ROOT_DIR)), 1, "Missing required contract file"))
            continue

        content = path.read_text(encoding="utf-8")
        markers = CONTRACT_REQUIRED_MARKERS.get(path, [])
        for marker in markers:
            if marker not in content:
                issues.append(
                    ContractIssue(
                        str(path.relative_to(ROOT_DIR)),
                        1,
                        f"Missing required contract marker: {marker}",
                    )
                )
    return issues


def is_non_authoritative(front_matter: Optional[Dict[str, Any]]) -> bool:
    if not front_matter:
        return False
    status = str(front_matter.get("status", "")).strip().lower()
    return status in NON_AUTHORITATIVE_STATUSES


def check_forbidden_active_contradictions(files: List[Path]) -> List[ContractIssue]:
    issues: List[ContractIssue] = []
    for path in files:
        if path.suffix.lower() != ".md":
            continue
        content = path.read_text(encoding="utf-8")
        front_matter = extract_front_matter(content)
        if is_non_authoritative(front_matter):
            continue

        for phrase in FORBIDDEN_ACTIVE_PHRASES:
            if phrase in content:
                issues.append(
                    ContractIssue(
                        str(path.relative_to(ROOT_DIR)),
                        find_line(content, phrase),
                        f"Forbidden active-doc contradiction found: '{phrase}'",
                    )
                )
    return issues


LINK_PATTERN = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")


def check_links(files: List[Path]) -> List[ContractIssue]:
    """Find [text](relative/path.md) links and verify targets exist."""
    issues: List[ContractIssue] = []
    for path in files:
        if path.suffix.lower() != ".md":
            continue
        content = path.read_text(encoding="utf-8")
        for match in LINK_PATTERN.finditer(content):
            target = match.group(2)
            if target.startswith("http") or target.startswith("#") or target.startswith("mailto:"):
                continue
            # Strip anchor fragments
            target_path = target.split("#")[0]
            if not target_path:
                continue
            resolved = (path.parent / target_path).resolve()
            if not resolved.exists():
                line_num = content[: match.start()].count("\n") + 1
                issues.append(
                    ContractIssue(
                        str(path.relative_to(ROOT_DIR)),
                        line_num,
                        f"Broken link: [{match.group(1)}]({target}) -> {resolved}",
                    )
                )
    return issues


def check_staleness(files: List[Path], threshold_days: int = 120) -> List[ContractIssue]:
    """Flag docs with last_updated/updated older than threshold."""
    issues: List[ContractIssue] = []
    today = datetime.date.today()
    for path in files:
        if path.suffix.lower() != ".md":
            continue
        content = path.read_text(encoding="utf-8")
        front_matter = extract_front_matter(content)
        if not front_matter:
            continue
        updated_raw = front_matter.get("last_updated") or front_matter.get("updated")
        if not updated_raw:
            continue
        try:
            if isinstance(updated_raw, datetime.date):
                updated = updated_raw
            else:
                updated = datetime.datetime.strptime(str(updated_raw), "%Y-%m-%d").date()
            age = (today - updated).days
            if age > threshold_days:
                issues.append(
                    ContractIssue(
                        str(path.relative_to(ROOT_DIR)),
                        1,
                        f"Stale document: last updated {updated} ({age} days ago, threshold={threshold_days})",
                    )
                )
        except (ValueError, TypeError):
            pass
    return issues


def run_contract_checks() -> List[ContractIssue]:
    issues: List[ContractIssue] = []
    issues.extend(check_contract_markers())
    docs = get_docs_files()
    issues.extend(check_forbidden_active_contradictions(docs))
    issues.extend(check_links(docs))
    issues.extend(check_staleness(docs))
    return issues


def validate_docs() -> Dict[str, Any]:
    docs = get_docs_files()
    duplicate_errors = detect_duplicate_artifacts(docs)
    results: Dict[str, Any] = {
        "total": len(docs),
        "valid": 0,
        "invalid": 0,
        "missing_front_matter": 0,
        "files": [],
    }

    for filepath in docs:
        content = filepath.read_text(encoding="utf-8")
        front_matter = extract_front_matter(content)
        errors = validate_front_matter(front_matter)
        errors.extend(duplicate_errors.get(filepath, []))

        fm_serializable = front_matter
        if front_matter and "updated" in front_matter and isinstance(front_matter["updated"], datetime.date):
            fm_serializable = dict(front_matter)
            fm_serializable["updated"] = str(front_matter["updated"])

        rel = str(filepath.relative_to(DOCS_DIR))
        file_result = {
            "path": rel,
            "valid": len(errors) == 0,
            "errors": errors,
            "front_matter": fm_serializable,
        }

        if front_matter is None:
            results["missing_front_matter"] += 1
        if errors:
            results["invalid"] += 1
        else:
            results["valid"] += 1

        results["files"].append(file_result)

    return results


def run_self_test() -> int:
    # Validate that contradiction detection catches active-doc phrases.
    tmp_path = ROOT_DIR / ".tmp_validate_docs_self_test.md"
    tmp_path.write_text(
        """---
status: active
title: tmp
description: temporary validation file for self-test
updated: 2026-02-23
---

Target: TypeScript/Bun ONLY. Zero Python. Zero Rust. Zero databases.

Active EGUI Workbench Track
""",
        encoding="utf-8",
    )

    tmp_dup_path = DOCS_DIR / ".tmp_validate_docs_duplicate 2.md"

    try:
        issues = check_forbidden_active_contradictions([tmp_path])
        if len(issues) < 2:
            print("SELF-TEST FAILED: expected contradiction issues not found")
            return 1

        tmp_dup_path.write_text(
            """---
title: tmp dup
description: temporary duplicate artifact file for self-test
type: doc
status: active
updated: 2026-02-23
---
""",
            encoding="utf-8",
        )
        dup = detect_duplicate_artifacts([tmp_dup_path])
        if tmp_dup_path not in dup:
            print("SELF-TEST FAILED: expected duplicate artifact issue not found")
            return 1

        print("SELF-TEST PASSED")
        return 0
    finally:
        if tmp_path.exists():
            tmp_path.unlink()
        if tmp_dup_path.exists():
            tmp_dup_path.unlink()


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate docs metadata and contract drift")
    parser.add_argument("--json", action="store_true", help="Output JSON")
    parser.add_argument("--contract", action="store_true", help="Run contract drift checks")
    parser.add_argument("--all", action="store_true", help="Run metadata and contract checks")
    parser.add_argument("--self-test", action="store_true", help="Run validator self-test")
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()

    run_meta = True
    run_contract = args.contract or args.all
    if args.contract and not args.all:
        run_meta = False

    payload: Dict[str, Any] = {}
    exit_code = 0

    if run_meta:
        meta_results = validate_docs()
        payload["metadata"] = meta_results
        if meta_results["invalid"] > 0:
            exit_code = 1

    if run_contract:
        contract_issues = run_contract_checks()
        payload["contract"] = {
            "valid": len(contract_issues) == 0,
            "issues": [issue.__dict__ for issue in contract_issues],
        }
        if contract_issues:
            exit_code = 1

    if args.json:
        print(json.dumps(payload, indent=2))
    else:
        if run_meta:
            m = payload["metadata"]
            print(f"Metadata validation: total={m['total']} valid={m['valid']} invalid={m['invalid']}")
            if m["invalid"] > 0:
                print("\nMetadata errors:")
                for f in m["files"]:
                    if not f["valid"]:
                        print(f"  {f['path']}")
                        for err in f["errors"]:
                            print(f"    - {err}")

        if run_contract:
            c = payload["contract"]
            print(f"Contract validation: {'PASS' if c['valid'] else 'FAIL'}")
            for issue in c["issues"]:
                print(f"  {issue['path']}:{issue['line']} - {issue['message']}")

    return exit_code


if __name__ == "__main__":
    sys.exit(main())
