# Local evidence / RAG layer

Agent-6 keeps retrieval **outside the Rust signal hot path**. The first implementation is an offline SQLite hybrid lexical/recency index in `research/agent6_research/evidence.py`; it requires no API key and cannot place orders or mutate model weights.

## Evidence contract

Every document carries a stable evidence ID plus source type/URI, source event timestamp, ingestion timestamp, optional symbol/timeframe, model/schema versions, SHA-256 content hash, and the factual content. Exact versions deduplicate; changed content receives a new evidence ID so history remains auditable.

Allowed inputs are validated project evidence such as normalized/replayed market summaries, signal lifecycle/outcome records, model-card/artifact manifests, evaluation/experiment reports, incidents/postmortems, and project documentation. Generated commentary is not ground truth and must not be recursively ingested as a training label.

## Leakage boundary

`retrieve(..., as_of_ms=T)` requires **both** `event_ts_ms <= T` and `ingestion_ts_ms <= T`. This prevents a historical evaluation from seeing a future event or a document that described an older event but was only ingested later. Symbol/timeframe filters are optional and deterministic.

Retrieval ranks lexical relevance with a bounded recency component and returns evidence IDs, hashes and provenance with every hit. Explanations should distinguish retrieved facts/citations from model inference.

## Deployment rule

Do not put retrieval on the low-latency Rust decision path until representative-session benchmarks show useful incremental value and acceptable latency. Initial consumers are research, post-trade diagnosis and asynchronous explanation/orchestrator context only.

## Validation

`pytest research/tests/test_evidence.py` covers future-event leakage, late-ingestion leakage, deduplication/versioning, deterministic ranking, metadata/citation preservation and fail-closed invalid metadata. Full research CI remains the release gate.
