"""Typed analytical storage for Agent-6 normalized market recordings.

The Rust JSONL recorder remains the source of truth. This offline module validates
and flattens those events into one typed Parquet dataset plus a DuckDB table.
"""
from __future__ import annotations

from hashlib import sha256
import json
import math
from pathlib import Path
from typing import Any

import duckdb
import polars as pl

SCHEMA_VERSION = "a6.market-events.v2"
KINDS = {"trade", "order_book", "ticker", "liquidation", "kline"}

SCHEMA = {
    "event_index": pl.UInt64,
    "kind": pl.String,
    "ts_ms": pl.UInt64,
    "symbol": pl.String,
    "interval": pl.String,
    "start_ms": pl.UInt64,
    "end_ms": pl.UInt64,
    "price": pl.Float64,
    "qty": pl.Float64,
    "side": pl.String,
    "update_id": pl.UInt64,
    "seq": pl.UInt64,
    "snapshot": pl.Boolean,
    "last_price": pl.Float64,
    "mark_price": pl.Float64,
    "index_price": pl.Float64,
    "open_interest": pl.Float64,
    "funding_rate": pl.Float64,
    "open": pl.Float64,
    "high": pl.Float64,
    "low": pl.Float64,
    "close": pl.Float64,
    "volume": pl.Float64,
    "turnover": pl.Float64,
    "confirmed": pl.Boolean,
    "bids_json": pl.String,
    "asks_json": pl.String,
}


def _finite(value: Any, name: str, *, positive: bool = False, non_negative: bool = False) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{name} must be numeric")
    out = float(value)
    if not math.isfinite(out):
        raise ValueError(f"{name} must be finite")
    if positive and out <= 0.0:
        raise ValueError(f"{name} must be positive")
    if non_negative and out < 0.0:
        raise ValueError(f"{name} must be non-negative")
    return out


def _optional_float(event: dict[str, Any], key: str, *, positive: bool = False, non_negative: bool = False) -> float | None:
    value = event.get(key)
    if value is None:
        return None
    return _finite(value, key, positive=positive, non_negative=non_negative)


def _u64(value: Any, name: str, *, positive: bool = False) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0 or (positive and value == 0):
        raise ValueError(f"{name} must be {'positive ' if positive else ''}unsigned integer")
    return value


def _levels(value: Any, name: str) -> list[list[float]]:
    if not isinstance(value, list):
        raise ValueError(f"{name} must be a list")
    rows: list[list[float]] = []
    for level in value:
        if not isinstance(level, list) or len(level) != 2:
            raise ValueError(f"malformed {name} level")
        rows.append([
            _finite(level[0], f"{name}.price", positive=True),
            _finite(level[1], f"{name}.qty", non_negative=True),
        ])
    return rows


def _base_row(index: int, event: dict[str, Any]) -> dict[str, Any]:
    kind = event.get("kind")
    if kind not in KINDS:
        raise ValueError(f"unsupported event kind at index {index}: {kind!r}")
    symbol = event.get("symbol")
    if not isinstance(symbol, str) or not symbol or len(symbol) > 32:
        raise ValueError(f"invalid symbol at index {index}")
    row = {name: None for name in SCHEMA}
    row.update(
        event_index=index,
        kind=kind,
        ts_ms=_u64(event.get("ts_ms"), "ts_ms", positive=True),
        symbol=symbol,
    )
    return row


def _flatten(index: int, event: dict[str, Any]) -> dict[str, Any]:
    row = _base_row(index, event)
    kind = row["kind"]

    if kind in {"trade", "liquidation"}:
        side = event.get("side")
        if side not in {"buy", "sell"}:
            raise ValueError(f"invalid side at index {index}")
        row.update(
            price=_finite(event.get("price"), "price", positive=True),
            qty=_finite(event.get("qty"), "qty", non_negative=True),
            side=side,
        )
    elif kind == "order_book":
        bids = _levels(event.get("bids"), "bids")
        asks = _levels(event.get("asks"), "asks")
        snapshot = event.get("snapshot")
        if not isinstance(snapshot, bool):
            raise ValueError(f"snapshot must be boolean at index {index}")
        row.update(
            update_id=_u64(event.get("update_id"), "update_id"),
            seq=_u64(event.get("seq"), "seq"),
            snapshot=snapshot,
            bids_json=json.dumps(bids, separators=(",", ":")),
            asks_json=json.dumps(asks, separators=(",", ":")),
        )
    elif kind == "ticker":
        row.update(
            last_price=_optional_float(event, "last_price", positive=True),
            mark_price=_optional_float(event, "mark_price", positive=True),
            index_price=_optional_float(event, "index_price", positive=True),
            open_interest=_optional_float(event, "open_interest", non_negative=True),
            funding_rate=_optional_float(event, "funding_rate"),
        )
    elif kind == "kline":
        start_ms = _u64(event.get("start_ms"), "start_ms", positive=True)
        end_ms = _u64(event.get("end_ms"), "end_ms")
        if end_ms < start_ms:
            raise ValueError(f"invalid kline bounds at index {index}")
        interval = event.get("interval")
        if interval not in {"1", "3", "5", "15"}:
            raise ValueError(f"unsupported kline interval at index {index}")
        o = _finite(event.get("open"), "open", positive=True)
        h = _finite(event.get("high"), "high", positive=True)
        l = _finite(event.get("low"), "low", positive=True)
        c = _finite(event.get("close"), "close", positive=True)
        if h < max(o, c) or l > min(o, c) or h < l:
            raise ValueError(f"invalid OHLC geometry at index {index}")
        confirmed = event.get("confirmed")
        if not isinstance(confirmed, bool):
            raise ValueError(f"confirmed must be boolean at index {index}")
        row.update(
            interval=interval,
            start_ms=start_ms,
            end_ms=end_ms,
            open=o,
            high=h,
            low=l,
            close=c,
            volume=_finite(event.get("volume"), "volume", non_negative=True),
            turnover=_finite(event.get("turnover"), "turnover", non_negative=True),
            confirmed=confirmed,
        )
    return row


def build_event_store(
    recording_path: str | Path,
    db_path: str | Path,
    parquet_path: str | Path,
    *,
    overwrite: bool = False,
) -> dict[str, Any]:
    """Build a deterministic typed analytical store from one normalized JSONL recording."""
    source = Path(recording_path)
    db = Path(db_path)
    parquet = Path(parquet_path)
    if not source.is_file():
        raise FileNotFoundError(source)
    if (db.exists() or parquet.exists()) and not overwrite:
        raise FileExistsError("output exists; pass overwrite=True to rebuild")

    payload = source.read_bytes()
    rows: list[dict[str, Any]] = []
    book_cursor: dict[str, tuple[bool, int, int]] = {}
    for line_no, raw in enumerate(payload.splitlines(), start=1):
        if not raw.strip():
            continue
        try:
            event = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise ValueError(f"invalid JSON at line {line_no}") from exc
        if not isinstance(event, dict):
            raise ValueError(f"event at line {line_no} must be an object")
        row = _flatten(len(rows), event)
        if row["kind"] == "order_book":
            seen, last_seq, last_update = book_cursor.get(row["symbol"], (False, 0, 0))
            seq, update_id = row["seq"], row["update_id"]
            if row["snapshot"] or update_id == 1:
                seen = True
            else:
                if not seen:
                    raise ValueError(f"order-book delta before snapshot at line {line_no}")
                if seq and last_seq and seq <= last_seq:
                    raise ValueError(f"non-monotonic order-book seq at line {line_no}")
                if update_id and last_update and update_id <= last_update:
                    raise ValueError(f"non-monotonic order-book update id at line {line_no}")
            book_cursor[row["symbol"]] = (seen, seq, update_id)
        rows.append(row)

    if not rows:
        raise ValueError("recording contains no normalized events")

    frame = pl.DataFrame(rows, schema=SCHEMA)
    db.parent.mkdir(parents=True, exist_ok=True)
    parquet.parent.mkdir(parents=True, exist_ok=True)
    if overwrite:
        db.unlink(missing_ok=True)
        parquet.unlink(missing_ok=True)

    frame.write_parquet(parquet, compression="zstd")
    con = duckdb.connect(str(db))
    try:
        parquet_sql = parquet.resolve().as_posix().replace("'", "''")
        con.execute(
            f"CREATE TABLE normalized_events AS SELECT * FROM read_parquet('{parquet_sql}') ORDER BY event_index"
        )
        con.execute("CREATE UNIQUE INDEX normalized_events_idx ON normalized_events(event_index)")
        count = int(con.execute("SELECT count(*) FROM normalized_events").fetchone()[0])
    finally:
        con.close()

    return {
        "schema_version": SCHEMA_VERSION,
        "source_sha256": sha256(payload).hexdigest(),
        "event_count": count,
        "min_ts_ms": min(row["ts_ms"] for row in rows),
        "max_ts_ms": max(row["ts_ms"] for row in rows),
        "parquet_path": str(parquet),
        "duckdb_path": str(db),
    }
