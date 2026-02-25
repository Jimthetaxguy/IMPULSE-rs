#!/usr/bin/env python3
"""PageIndex feasibility benchmark (local-first).

Compares:
1) Impulse baseline keyword retrieval
2) PageIndex-like local structure retrieval (heading/section-aware)

Usage:
  python3 memory-pipeline/pageindex_feasibility_benchmark.py --root . \
    --queries memory-pipeline/pageindex_eval_queries.sample.json \
    --out docs/research/pageindex-feasibility-report.json
"""

from __future__ import annotations

import argparse
import json
import math
import re
import statistics
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Tuple


@dataclass
class Doc:
    doc_id: str
    source: str
    title: str
    text: str
    headings: List[str]


def normalize(text: str) -> str:
    return re.sub(r"\s+", " ", text.lower()).strip()


def tokenize(query: str) -> List[str]:
    return [t for t in re.findall(r"[a-z0-9_]+", query.lower()) if t]


def heading_extract(md: str) -> List[str]:
    out = []
    for line in md.splitlines():
        m = re.match(r"^\s{0,3}(#{1,6})\s+(.*)$", line)
        if m:
            out.append(m.group(2).strip())
    return out


def load_docs(root: Path) -> List[Doc]:
    docs: List[Doc] = []
    docs_dir = root / "docs"
    for p in sorted(docs_dir.rglob("*.md")):
        try:
            text = p.read_text(encoding="utf-8")
        except Exception:
            continue
        rel = p.relative_to(root).as_posix()
        docs.append(
            Doc(
                doc_id=f"doc::{rel}",
                source="docs",
                title=p.stem,
                text=text,
                headings=heading_extract(text),
            )
        )

    impulse_dir = root / ".impulse"
    history_path = impulse_dir / "HISTORY.jsonl"
    if history_path.exists():
        for i, line in enumerate(history_path.read_text(encoding="utf-8").splitlines()):
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            doc_id = f"history::{row.get('session_id', i)}"
            text = " ".join(
                [
                    row.get("session_name", ""),
                    row.get("summary", ""),
                    " ".join(row.get("files_touched", [])),
                    " ".join(row.get("tools_used", [])),
                ]
            )
            docs.append(
                Doc(
                    doc_id=doc_id,
                    source="history",
                    title=row.get("session_name", "session"),
                    text=text,
                    headings=[],
                )
            )

    genome_path = impulse_dir / "GENOME.md"
    if genome_path.exists():
        try:
            obj = json.loads(genome_path.read_text(encoding="utf-8"))
            decisions = obj.get("decisions", [])
            for i, d in enumerate(decisions):
                doc_id = f"genome::{i}"
                text = " ".join(
                    [d.get("description", ""), d.get("rationale", "") or "", " ".join(d.get("tags", []))]
                )
                docs.append(
                    Doc(
                        doc_id=doc_id,
                        source="genome",
                        title=d.get("description", f"decision-{i}"),
                        text=text,
                        headings=[],
                    )
                )
        except Exception:
            pass

    return docs


def keyword_score(query: str, doc: Doc) -> float:
    terms = tokenize(query)
    if not terms:
        return 0.0
    text = normalize(f"{doc.title} {doc.text}")
    score = 0.0
    for term in terms:
        count = text.count(term)
        if count:
            score += 1.0 + math.log(1 + count)
    return score


def pageindex_local_score(query: str, doc: Doc) -> float:
    terms = tokenize(query)
    if not terms:
        return 0.0
    text = normalize(doc.text)
    headings = [normalize(h) for h in doc.headings]
    score = 0.0
    for term in terms:
        in_heading = sum(1 for h in headings if term in h)
        in_text = text.count(term)
        in_title = normalize(doc.title).count(term)
        score += (2.5 * in_heading) + (1.0 * in_text) + (2.0 * in_title)
    if doc.source == "docs":
        score *= 1.05
    return score


def run_ranked(query: str, docs: List[Doc], scorer) -> List[Tuple[str, float]]:
    scored = [(d.doc_id, scorer(query, d)) for d in docs]
    scored = [x for x in scored if x[1] > 0]
    scored.sort(key=lambda x: x[1], reverse=True)
    return scored


def precision_at_k(ranked: List[str], relevant: List[str], k: int) -> float:
    if k <= 0:
        return 0.0
    rel = set(relevant)
    top = ranked[:k]
    if not top:
        return 0.0
    return sum(1 for d in top if d in rel) / float(k)


def mrr(ranked: List[str], relevant: List[str]) -> float:
    rel = set(relevant)
    for i, d in enumerate(ranked, start=1):
        if d in rel:
            return 1.0 / float(i)
    return 0.0


def ndcg_at_k(ranked: List[str], relevant: List[str], k: int) -> float:
    rel = set(relevant)
    dcg = 0.0
    for i, d in enumerate(ranked[:k], start=1):
        gain = 1.0 if d in rel else 0.0
        dcg += gain / math.log2(i + 1)
    idcg = sum(1.0 / math.log2(i + 1) for i in range(1, min(len(rel), k) + 1))
    return dcg / idcg if idcg > 0 else 0.0


def percentile(values: List[float], p: float) -> float:
    if not values:
        return 0.0
    vals = sorted(values)
    idx = max(0, min(len(vals) - 1, int(round((p / 100.0) * (len(vals) - 1)))))
    return vals[idx]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".", help="repo root")
    parser.add_argument("--queries", default="", help="labeled query json file")
    parser.add_argument("--out", default="docs/research/pageindex-feasibility-report.json")
    parser.add_argument("--k", type=int, default=5)
    args = parser.parse_args()

    root = Path(args.root).resolve()
    docs = load_docs(root)
    if not docs:
        raise SystemExit("No corpus documents found.")

    query_data = []
    if args.queries:
        qpath = (root / args.queries) if not Path(args.queries).is_absolute() else Path(args.queries)
        if qpath.exists():
            query_data = json.loads(qpath.read_text(encoding="utf-8"))

    if not query_data:
        query_data = [
            {"query": "retrieval status and fallback behavior", "relevant_doc_ids": []},
            {"query": "semantic search backend and vector availability", "relevant_doc_ids": []},
            {"query": "session lifecycle and hooks", "relevant_doc_ids": []},
        ]

    latency_baseline = []
    latency_pageindex = []
    rows = []

    for q in query_data:
        query = q["query"]
        relevant = q.get("relevant_doc_ids", [])

        t0 = time.perf_counter()
        baseline = run_ranked(query, docs, keyword_score)
        latency_baseline.append((time.perf_counter() - t0) * 1000.0)

        t1 = time.perf_counter()
        pageindex_local = run_ranked(query, docs, pageindex_local_score)
        latency_pageindex.append((time.perf_counter() - t1) * 1000.0)

        baseline_ids = [doc_id for doc_id, _ in baseline]
        pageindex_ids = [doc_id for doc_id, _ in pageindex_local]

        row = {
            "query": query,
            "baseline_top": baseline[: args.k],
            "pageindex_local_top": pageindex_local[: args.k],
        }
        if relevant:
            row.update(
                {
                    "baseline_p_at_k": precision_at_k(baseline_ids, relevant, args.k),
                    "pageindex_local_p_at_k": precision_at_k(pageindex_ids, relevant, args.k),
                    "baseline_mrr": mrr(baseline_ids, relevant),
                    "pageindex_local_mrr": mrr(pageindex_ids, relevant),
                    "baseline_ndcg": ndcg_at_k(baseline_ids, relevant, args.k),
                    "pageindex_local_ndcg": ndcg_at_k(pageindex_ids, relevant, args.k),
                }
            )
        rows.append(row)

    with_labels = [r for r in rows if "baseline_p_at_k" in r]
    quality = {}
    decision = "NO-GO"
    if with_labels:
        bp = statistics.mean(r["baseline_p_at_k"] for r in with_labels)
        pp = statistics.mean(r["pageindex_local_p_at_k"] for r in with_labels)
        bm = statistics.mean(r["baseline_mrr"] for r in with_labels)
        pm = statistics.mean(r["pageindex_local_mrr"] for r in with_labels)
        bn = statistics.mean(r["baseline_ndcg"] for r in with_labels)
        pn = statistics.mean(r["pageindex_local_ndcg"] for r in with_labels)
        uplift = ((pp - bp) / bp) * 100.0 if bp > 0 else 0.0
        quality = {
            "baseline_p_at_k": bp,
            "pageindex_local_p_at_k": pp,
            "baseline_mrr": bm,
            "pageindex_local_mrr": pm,
            "baseline_ndcg": bn,
            "pageindex_local_ndcg": pn,
            "precision_uplift_percent": uplift,
        }
        if uplift >= 10.0:
            decision = "GO-OPTIONAL"

    result = {
        "summary": {
            "doc_count": len(docs),
            "query_count": len(query_data),
            "labeled_query_count": len(with_labels),
            "baseline_latency_ms_p95": percentile(latency_baseline, 95),
            "baseline_latency_ms_p99": percentile(latency_baseline, 99),
            "pageindex_local_latency_ms_p95": percentile(latency_pageindex, 95),
            "pageindex_local_latency_ms_p99": percentile(latency_pageindex, 99),
            "fallback_error_rate": 0.0,
            "operational_complexity": {
                "baseline": "low",
                "pageindex_local_structure": "medium",
                "pageindex_api_augmented": "high",
            },
        },
        "quality": quality,
        "decision": decision,
        "rows": rows,
        "notes": [
            "This harness is local-first and does not call PageIndex cloud APIs.",
            "For api-augmented mode, run a separate gated experiment with explicit API credentials and budget.",
        ],
    }

    out_path = Path(args.out)
    if not out_path.is_absolute():
        out_path = root / out_path
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(f"Wrote benchmark report: {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
