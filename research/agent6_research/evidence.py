"""Offline, leakage-safe evidence retrieval for Agent-6 research and explanations.

This module is intentionally outside the Rust decision hot path. It stores only caller-supplied,
trusted project evidence and enforces event-time/ingestion-time cutoffs during retrieval.
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
TRUSTED_SOURCE_TYPES = frozenset({
    "normalized_market_summary", "signal_lifecycle", "evaluation", "experiment",
    "model_card", "artifact_manifest", "incident", "postmortem", "project_doc",
    "regime_summary",
})
EVIDENCE_KINDS = frozenset({"fact", "inference"})


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
    evidence_kind: str = "fact"
    validated_label: bool = False

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
    evidence_kind: str
    validated_label: bool


class EvidenceIndex:
    """Small local lexical/recency index with strict point-in-time retrieval.

    It is deliberately secret-free and asynchronous to the Rust hot path. Generated/model
    commentary must be marked ``inference`` and is excluded from factual retrieval by default.
    ``validated_label`` is metadata for downstream research; this class never turns inference into
    a training label automatically.
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
                evidence_kind TEXT NOT NULL DEFAULT 'fact',
                validated_label INTEGER NOT NULL DEFAULT 0,
                UNIQUE(source_type, source_uri, event_ts_ms, content_hash)
            )"""
        )
        columns = {r[1] for r in self.db.execute("PRAGMA table_info(evidence)")}
        if "evidence_kind" not in columns:
            self.db.execute("ALTER TABLE evidence ADD COLUMN evidence_kind TEXT NOT NULL DEFAULT 'fact'")
        if "validated_label" not in columns:
            self.db.execute("ALTER TABLE evidence ADD COLUMN validated_label INTEGER NOT NULL DEFAULT 0")
        self.db.execute("CREATE INDEX IF NOT EXISTS evidence_asof ON evidence(event_ts_ms, ingestion_ts_ms)")
        self.db.commit()

    def close(self) -> None:
        self.db.close()

    def ingest(self, doc: EvidenceDocument) -> str:
        if doc.source_type not in TRUSTED_SOURCE_TYPES:
            raise ValueError(f"untrusted evidence source_type: {doc.source_type}")
        if not doc.source_uri.strip() or not doc.content.strip():
            raise ValueError("source_uri and content are required")
        if doc.event_ts_ms < 0 or doc.ingestion_ts_ms < 0:
            raise ValueError("timestamps must be non-negative")
        if doc.ingestion_ts_ms < doc.event_ts_ms:
            raise ValueError("ingestion timestamp cannot predate source/event timestamp")
        if doc.evidence_kind not in EVIDENCE_KINDS:
            raise ValueError("evidence_kind must be fact or inference")
        if doc.evidence_kind == "inference" and doc.validated_label:
            raise ValueError("model inference cannot be marked as a validated training label")
        self.db.execute(
            """INSERT OR IGNORE INTO evidence
            (evidence_id,source_type,source_uri,event_ts_ms,ingestion_ts_ms,symbol,timeframe,
             model_version,schema_version,content_hash,content,evidence_kind,validated_label)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)""",
            (doc.evidence_id, doc.source_type, doc.source_uri, doc.event_ts_ms, doc.ingestion_ts_ms,
             doc.symbol, doc.timeframe, doc.model_version, doc.schema_version, doc.content_hash,
             doc.content, doc.evidence_kind, int(doc.validated_label)),
        )
        self.db.commit()
        return doc.evidence_id

    def retrieve(self, query: str, *, as_of_ms: int, limit: int = 8,
                 symbol: str | None = None, timeframe: str | None = None,
                 include_inference: bool = False) -> list[EvidenceHit]:
        if as_of_ms < 0 or limit <= 0:
            raise ValueError("as_of_ms must be non-negative and limit positive")
        qtokens = set(_tokens(query))
        if not qtokens:
            return []
        sql = ("SELECT evidence_id,source_type,source_uri,event_ts_ms,ingestion_ts_ms,content_hash,"
               "content,symbol,timeframe,model_version,schema_version,evidence_kind,validated_label "
               "FROM evidence WHERE event_ts_ms<=? AND ingestion_ts_ms<=?")
        args: list[object] = [as_of_ms, as_of_ms]
        if not include_inference:
            sql += " AND evidence_kind='fact'"
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
            hits.append(EvidenceHit(row[0], score, row[1], row[2], row[3], row[4], row[5], row[6],
                                    row[7], row[8], row[9], row[10], row[11], bool(row[12])))
        hits.sort(key=lambda h: (-h.score, -h.event_ts_ms, h.evidence_id))
        return hits[:limit]


def _tokens(text: str) -> Iterable[str]:
    return _TOKEN.findall(text.lower())
