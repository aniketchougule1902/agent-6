from agent6_research.evidence import EvidenceDocument, EvidenceIndex


def doc(content: str, event: int, ingest: int, *, uri: str = "recording://session-1") -> EvidenceDocument:
    return EvidenceDocument(
        source_type="normalized_market_summary", source_uri=uri,
        event_ts_ms=event, ingestion_ts_ms=ingest, content=content,
        symbol="BTCUSDT", timeframe="1", model_version="champion-v1", schema_version="events-v2",
    )


def test_asof_blocks_future_event_and_late_ingestion(tmp_path):
    idx = EvidenceIndex(tmp_path / "evidence.sqlite")
    past = idx.ingest(doc("BTC order flow imbalance bullish", 1_000, 1_100))
    idx.ingest(doc("BTC order flow imbalance bearish future", 2_000, 2_000, uri="recording://future"))
    idx.ingest(doc("BTC order flow imbalance late note", 900, 2_500, uri="incident://late"))
    hits = idx.retrieve("order flow imbalance", as_of_ms=1_500, symbol="BTCUSDT", timeframe="1")
    assert [h.evidence_id for h in hits] == [past]
    assert all(h.event_ts_ms <= 1_500 and h.ingestion_ts_ms <= 1_500 for h in hits)
    idx.close()


def test_deduplicates_exact_version_and_preserves_changed_content(tmp_path):
    idx = EvidenceIndex(tmp_path / "evidence.sqlite")
    first = doc("calibration report brier stable", 1_000, 1_100, uri="report://calibration")
    assert idx.ingest(first) == idx.ingest(first)
    changed = doc("calibration report brier improved", 1_000, 1_200, uri="report://calibration")
    assert idx.ingest(changed) != first.evidence_id
    hits = idx.retrieve("calibration brier", as_of_ms=2_000)
    assert len(hits) == 2
    assert len({h.content_hash for h in hits}) == 2
    idx.close()


def test_retrieval_is_deterministic_filtered_and_citable(tmp_path):
    idx = EvidenceIndex(tmp_path / "evidence.sqlite")
    wanted = idx.ingest(doc("volatile regime spread widening", 3_000, 3_000))
    other = EvidenceDocument(
        source_type="evaluation", source_uri="eval://eth", event_ts_ms=3_000, ingestion_ts_ms=3_000,
        content="volatile regime spread widening", symbol="ETHUSDT", timeframe="1",
        model_version="challenger-v2", schema_version="eval-v1",
    )
    idx.ingest(other)
    a = idx.retrieve("volatile spread", as_of_ms=4_000, symbol="BTCUSDT", limit=3)
    b = idx.retrieve("volatile spread", as_of_ms=4_000, symbol="BTCUSDT", limit=3)
    assert a == b and [h.evidence_id for h in a] == [wanted]
    assert a[0].source_uri == "recording://session-1"
    assert a[0].model_version == "champion-v1"
    assert a[0].schema_version == "events-v2"
    idx.close()


def test_rejects_invalid_metadata(tmp_path):
    idx = EvidenceIndex(tmp_path / "evidence.sqlite")
    bad = EvidenceDocument("", "x", 0, 0, "content")
    try:
        idx.ingest(bad)
        assert False, "expected validation failure"
    except ValueError:
        pass
    try:
        idx.retrieve("x", as_of_ms=-1)
        assert False, "expected validation failure"
    except ValueError:
        pass
    idx.close()
