# Local Evidence / RAG Safety Boundary

Agent-6's evidence index is an offline research/explanation component. It is deliberately not on the deterministic Rust signal hot path and requires no external API or secret.

Only explicit project evidence classes are accepted: normalized market/regime summaries, signal lifecycle records, evaluation/experiment reports, model cards/artifact manifests, incidents/postmortems, and project documentation. Every record carries source URI/type, source/event time, ingestion time, optional symbol/timeframe/model/schema version, SHA-256 content hash, and a stable evidence ID.

Point-in-time retrieval enforces both `event_ts_ms <= T` and `ingestion_ts_ms <= T`. An observation cannot be ingested before its event timestamp. This prevents a replay/evaluation at T from retrieving evidence that was generated, discovered, or backfilled later.

Evidence is explicitly classified as `fact` or `inference`. Factual retrieval excludes inference by default. Model/generated commentary may be retained as inference for explanation audit, but cannot be marked as a validated training label and is never recursively promoted to ground truth by the index.

Retrieval currently uses deterministic lexical overlap plus recency ranking. This keeps the dependency footprint small and behavior reproducible. Do not put it on the low-latency decision path until a representative corpus benchmark demonstrates useful retrieval quality and acceptable latency. Citations should expose the returned `evidence_id`, source URI, content hash, and timestamps so an explanation can be audited.

The index is version-friendly: exact source/event/content duplicates are idempotent, while changed content receives a different hash/evidence ID. SQLite WAL + FULL synchronous mode is used for local durability. Existing databases are migrated with the evidence classification fields on open.
