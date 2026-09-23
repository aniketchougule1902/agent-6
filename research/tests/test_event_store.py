import json

import duckdb
import pytest

from agent6_research.event_store import SCHEMA_VERSION, build_event_store


def _write_recording(path):
    events = [
        {"kind":"order_book","ts_ms":1000,"symbol":"BTCUSDT","update_id":1,"seq":10,"snapshot":True,"bids":[[100.0,2.0]],"asks":[[101.0,3.0]]},
        {"kind":"order_book","ts_ms":1001,"symbol":"BTCUSDT","update_id":2,"seq":11,"snapshot":False,"bids":[[100.0,0.0],[99.5,1.2]],"asks":[]},
        {"kind":"trade","ts_ms":1002,"symbol":"BTCUSDT","price":100.5,"qty":0.2,"side":"buy"},
        {"kind":"ticker","ts_ms":1003,"symbol":"BTCUSDT","last_price":100.5,"mark_price":100.4,"index_price":100.3,"open_interest":10000.0,"funding_rate":0.0001},
        {"kind":"liquidation","ts_ms":1004,"symbol":"BTCUSDT","price":99.0,"qty":1.5,"side":"sell"},
        {"kind":"kline","ts_ms":1050,"symbol":"BTCUSDT","interval":"1","start_ms":1000,"end_ms":59999,"open":100.0,"high":102.0,"low":99.0,"close":101.0,"volume":12.0,"turnover":1212.0,"confirmed":True},
    ]
    path.write_text("\n".join(json.dumps(x, separators=(",",":")) for x in events) + "\n", encoding="utf-8")
    return events


def test_typed_store_round_trip(tmp_path):
    source = tmp_path / "market.jsonl"
    events = _write_recording(source)
    db = tmp_path / "market.duckdb"
    parquet = tmp_path / "market.parquet"

    manifest = build_event_store(source, db, parquet)

    assert manifest["schema_version"] == SCHEMA_VERSION
    assert manifest["event_count"] == len(events)
    assert len(manifest["source_sha256"]) == 64
    assert db.exists()
    assert parquet.exists()

    con = duckdb.connect(str(db), read_only=True)
    try:
        kinds = [row[0] for row in con.execute(
            "SELECT kind FROM normalized_events ORDER BY event_index"
        ).fetchall()]
        assert kinds == [event["kind"] for event in events]

        kline = con.execute(
            "SELECT start_ms,end_ms,open,high,low,close FROM normalized_events WHERE kind='kline'"
        ).fetchone()
        assert kline == (1000, 59999, 100.0, 102.0, 99.0, 101.0)

        book_delta = con.execute(
            "SELECT bids_json FROM normalized_events WHERE event_index=1"
        ).fetchone()[0]
        assert json.loads(book_delta)[0] == [100.0, 0.0]
    finally:
        con.close()


def test_rejects_non_monotonic_book_without_output(tmp_path):
    source = tmp_path / "bad.jsonl"
    source.write_text(
        "\n".join([
            json.dumps({"kind":"order_book","ts_ms":1,"symbol":"BTCUSDT","update_id":1,"seq":10,"snapshot":True,"bids":[],"asks":[]}),
            json.dumps({"kind":"order_book","ts_ms":2,"symbol":"BTCUSDT","update_id":2,"seq":10,"snapshot":False,"bids":[],"asks":[]}),
        ]) + "\n",
        encoding="utf-8",
    )
    db = tmp_path / "bad.duckdb"
    parquet = tmp_path / "bad.parquet"
    with pytest.raises(ValueError, match="non-monotonic order-book seq"):
        build_event_store(source, db, parquet)
    assert not db.exists()
    assert not parquet.exists()


def test_rejects_legacy_kline_without_exact_bounds(tmp_path):
    source = tmp_path / "legacy.jsonl"
    source.write_text(
        json.dumps({
            "kind":"kline","ts_ms":1,"symbol":"BTCUSDT","interval":"1",
            "open":100.0,"high":101.0,"low":99.0,"close":100.0,
            "volume":1.0,"turnover":100.0,"confirmed":True
        }) + "\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="start_ms"):
        build_event_store(source, tmp_path / "legacy.duckdb", tmp_path / "legacy.parquet")
