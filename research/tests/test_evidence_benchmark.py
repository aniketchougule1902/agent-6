import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from benchmark_evidence import load_queries, run  # noqa: E402
from agent6_research.evidence import EvidenceDocument, EvidenceIndex


def test_benchmark_measures_quality_latency_and_point_in_time_safety(tmp_path):
    db = tmp_path / "evidence.sqlite"
    idx = EvidenceIndex(db)
    relevant = idx.ingest(EvidenceDocument(
        source_type="evaluation", source_uri="eval://walk-forward/1",
        event_ts_ms=1_000, ingestion_ts_ms=1_100,
        content="BTCUSDT volatile regime after cost expectancy positive",
        symbol="BTCUSDT", timeframe="1", model_version="champion-v1", schema_version="eval-v1",
    ))
    idx.ingest(EvidenceDocument(
        source_type="evaluation", source_uri="eval://future/2",
        event_ts_ms=3_000, ingestion_ts_ms=3_100,
        content="BTCUSDT volatile regime after cost expectancy future",
        symbol="BTCUSDT", timeframe="1", model_version="challenger-v2", schema_version="eval-v1",
    ))
    idx.ingest(EvidenceDocument(
        source_type="experiment", source_uri="analysis://model/1",
        event_ts_ms=900, ingestion_ts_ms=1_000,
        content="BTCUSDT volatile regime model commentary",
        symbol="BTCUSDT", timeframe="1", evidence_kind="inference",
    ))
    idx.close()

    queries = tmp_path / "queries.jsonl"
    queries.write_text(json.dumps({
        "query": "volatile regime expectancy", "as_of_ms": 2_000,
        "symbol": "BTCUSDT", "timeframe": "1", "limit": 5,
        "relevant_evidence_ids": [relevant],
    }) + "\n", encoding="utf-8")
    report = run(db, queries)
    assert report["queries"] == report["judged_queries"] == 1
    assert report["quality"]["hit_rate_at_k"] == 1.0
    assert report["quality"]["mrr"] == 1.0
    assert report["latency_ms"]["p95"] >= 0.0
    assert report["safety"] == {"future_leakage_queries": 0, "inference_leakage_queries": 0}
    assert report["hot_path"] is False


def test_benchmark_query_manifest_fails_closed(tmp_path):
    empty = tmp_path / "empty.jsonl"
    empty.write_text("", encoding="utf-8")
    try:
        load_queries(empty)
        assert False, "empty benchmark must fail"
    except ValueError:
        pass

    malformed = tmp_path / "bad.jsonl"
    malformed.write_text(json.dumps({"query": "x", "as_of_ms": -1}) + "\n", encoding="utf-8")
    try:
        load_queries(malformed)
        assert False, "negative point-in-time cutoff must fail"
    except ValueError:
        pass
