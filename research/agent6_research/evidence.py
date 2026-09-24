"""Offline, leakage-safe evidence retrieval for Agent-6 research and explanations.

This module is intentionally outside the Rust decision hot path. It stores only caller-supplied
validated evidence and enforces event-time/ingestion-time cutoffs during retrieval.
"""
from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
import math
import re
import sqlite3
from pathlib import Path
from typing import Iterable

_TOKEN = re.compile(r"[a-z0-9_]{2,}")


@dataclass(frozen=True)
class EvidenceDocument:
    source_type: str
    source_uri: str
    event_ts_ms: int
    ingestion_ts_ms: int
    content: str
    symbol: str | None = None
    timeframe: str | None = None
    model_version: str | None = None
    schema_version: str | None = None

    @property
    def content_hash(self) -> str:
        return sha256(self.content.encode("utf-8")).hexdigest()

    @property
    def evidence_id(self) -> str:
        identity = "\x1f".join((self.source_type, self.source_uri, str(self.event_ts_ms), self.content_hash))
        return "ev_" + sha256(identity.encode("utf-8")).hexdigest()[:24]


@dataclass(frozen=True)
class EvidenceHit:
    evidence_id: str
    score: float
    source_type: str
    source_uri: str
    event_ts_ms: int
    ingestion_ts_ms: int
    content_hash: str
    content: str
    symbol: str | None
    timeframe: str | None
    model_version: str | None
    schema_version: str | None


class EvidenceIndex:
    """Small local hybrid lexical/recency index with strict as-of retrieval.

    SQLite keeps the index portable and secret-free. Generated commentary should not be ingested
    unless a caller explicitly classifies it as validated evidence; this class never self-ingests
    retrieval output or model text.
    """

    def __init__(self, path: str | Path):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(self.path)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.execute(
            """CREATE TABLE IF NOT EXISTS evidence (
                evidence_id TEXT PRIMARY KEY,
                source_type TEXT NOT NULL, source_uri TEXT NOT NULL,
                event_ts_ms INTEGER NOT NULL, ingestion_ts_ms INTEGER NOT NULL,
                symbol TEXT, timeframe TEXT, model_version TEXT, schema_version TEXT,
                content_hash TEXT NOT NULL, content TEXT NOT NULL,
                UNIQUE(source_type, source_uri, event_ts_ms, content_hash)
            )"""
        )
        self.db.execute("CREATE INDEX IF NOT EXISTS evidence_asof ON evidence(event_ts_ms, ingestion_ts_ms)")
        self.db.commit()

    def close(self) -> None:
        self.db.close()

    def ingest(self, doc: EvidenceDocument) -> str:
        if not doc.source_type.strip() or not doc.source_uri.strip() or not doc.content.strip():
            raise ValueError("source_type, source_uri and content are required")
        if doc.event_ts_ms < 0 or doc.ingestion_ts_ms < 0:
            raise ValueError("timestamps must be non-negative")
        self.db.execute(
            """INSERT OR IGNORE INTO evidence
            (evidence_id,source_type,source_uri,event_ts_ms,ingestion_ts_ms,symbol,timeframe,
             model_version,schema_version,content_hash,content) VALUES (?,?,?,?,?,?,?,?,?,?,?)""",
            (doc.evidence_id, doc.source_type, doc.source_uri, doc.event_ts_ms, doc.ingestion_ts_ms,
             doc.symbol, doc.timeframe, doc.model_version, doc.schema_version, doc.content_hash, doc.content),
        )
        self.db.commit()
        return doc.evidence_id

    def retrieve(self, query: str, *, as_of_ms: int, limit: int = 8,
                 symbol: str | None = None, timeframe: str | None = None) -> list[EvidenceHit]:
        if as_of_ms < 0 or limit <= 0:
            raise ValueError("as_of_ms must be non-negative and limit positive")
        qtokens = set(_tokens(query))
        if not qtokens:
            return []
        sql = "SELECT evidence_id,source_type,source_uri,event_ts_ms,ingestion_ts_ms,content_hash,content,symbol,timeframe,model_version,schema_version FROM evidence WHERE event_ts_ms<=? AND ingestion_ts_ms<=?"
        args: list[object] = [as_of_ms, as_of_ms]
        if symbol is not None:
            sql += " AND symbol=?"; args.append(symbol)
        if timeframe is not None:
            sql += " AND timeframe=?"; args.append(timeframe)
        rows = self.db.execute(sql, args).fetchall()
        hits: list[EvidenceHit] = []
        for row in rows:
            tokens = set(_tokens(row[6]))
            overlap = len(qtokens & tokens)
            if overlap == 0:
                continue
            lexical = overlap / math.sqrt(len(qtokens) * max(1, len(tokens)))
            age_hours = max(0.0, (as_of_ms - row[3]) / 3_600_000.0)
            recency = 1.0 / (1.0 + age_hours / 24.0)
            score = 0.85 * lexical + 0.15 * recency
            hits.append(EvidenceHit(row[0], score, row[1], row[2], row[3], row[4], row[5], row[6], row[7], row[8], row[9], row[10]))
        hits.sort(key=lambda h: (-h.score, -h.event_ts_ms, h.evidence_id))
        return hits[:limit]


def _tokens(text: str) -> Iterable[str]:
    return _TOKEN.findall(text.lower())
