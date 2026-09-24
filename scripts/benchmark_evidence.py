#!/usr/bin/env python3
"""Benchmark Agent-6's local evidence retrieval without touching the signal hot path.

Input is JSONL. Each row must contain `query` and `as_of_ms`; optional fields are
`symbol`, `timeframe`, `limit`, `include_inference`, and `relevant_evidence_ids`.
The latter enables deterministic retrieval-quality measurement (hit-rate/MRR).
This tool never ingests evidence and therefore cannot introduce future data.
"""
from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "research"))
from agent6_research.evidence import EvidenceIndex  # noqa: E402


def percentile(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    pos = (len(ordered) - 1) * q
    lo, hi = math.floor(pos), math.ceil(pos)
    if lo == hi:
        return ordered[lo]
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (pos - lo)


def load_queries(path: Path) -> list[dict]:
    rows: list[dict] = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        row = json.loads(line)
        if not isinstance(row.get("query"), str) or not row["query"].strip():
            raise ValueError(f"line {line_no}: non-empty query required")
        if not isinstance(row.get("as_of_ms"), int) or row["as_of_ms"] < 0:
            raise ValueError(f"line {line_no}: non-negative integer as_of_ms required")
        relevant = row.get("relevant_evidence_ids", [])
        if not isinstance(relevant, list) or any(not isinstance(x, str) or not x for x in relevant):
            raise ValueError(f"line {line_no}: relevant_evidence_ids must be a string list")
        rows.append(row)
    if not rows:
        raise ValueError("benchmark query file is empty")
    return rows


def run(index_path: Path, query_path: Path) -> dict:
    rows = load_queries(query_path)
    idx = EvidenceIndex(index_path)
    latencies: list[float] = []
    reciprocal_ranks: list[float] = []
    hits_at_k = 0
    judged = 0
    future_leakage = 0
    inference_leakage = 0
    try:
        for row in rows:
            start = time.perf_counter_ns()
            hits = idx.retrieve(
                row["query"], as_of_ms=row["as_of_ms"], limit=int(row.get("limit", 8)),
                symbol=row.get("symbol"), timeframe=row.get("timeframe"),
                include_inference=bool(row.get("include_inference", False)),
            )
            latencies.append((time.perf_counter_ns() - start) / 1_000_000.0)
            if any(h.event_ts_ms > row["as_of_ms"] or h.ingestion_ts_ms > row["as_of_ms"] for h in hits):
                future_leakage += 1
            if not row.get("include_inference", False) and any(h.evidence_kind != "fact" for h in hits):
                inference_leakage += 1
            relevant = set(row.get("relevant_evidence_ids", []))
            if relevant:
                judged += 1
                rank = next((i for i, h in enumerate(hits, 1) if h.evidence_id in relevant), None)
                if rank is not None:
                    hits_at_k += 1
                    reciprocal_ranks.append(1.0 / rank)
                else:
                    reciprocal_ranks.append(0.0)
    finally:
        idx.close()
    return {
        "schema_version": 1,
        "queries": len(rows),
        "judged_queries": judged,
        "latency_ms": {
            "mean": statistics.fmean(latencies),
            "p50": percentile(latencies, 0.50),
            "p95": percentile(latencies, 0.95),
            "max": max(latencies),
        },
        "quality": {
            "hit_rate_at_k": (hits_at_k / judged) if judged else None,
            "mrr": statistics.fmean(reciprocal_ranks) if judged else None,
        },
        "safety": {
            "future_leakage_queries": future_leakage,
            "inference_leakage_queries": inference_leakage,
        },
        "hot_path": False,
    }


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--index", required=True, type=Path)
    p.add_argument("--queries", required=True, type=Path)
    p.add_argument("--output", type=Path)
    args = p.parse_args()
    report = run(args.index, args.queries)
    encoded = json.dumps(report, indent=2, sort_keys=True)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded + "\n", encoding="utf-8")
    print(encoded)
    if report["safety"]["future_leakage_queries"] or report["safety"]["inference_leakage_queries"]:
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
