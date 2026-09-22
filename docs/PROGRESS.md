# Progress Log

## 2026-09-22 — Bootstrap

### Completed

- Initialized empty `agent-6` repository.
- Added local-only architecture and setup README.
- Added Rust event-driven core.
- Added Bybit V5 live linear-perpetual market gateway.
- Added REST historical kline backfill.
- Added 1m/3m/5m/15m market state.
- Added real-time trades, exact L50 snapshot/delta local-book reconstruction, derivatives ticker and liquidation ingestion.
- Added L50 and top-5 imbalance plus microprice displacement.
- Added heartbeat and automatic reconnect loop.
- Added stale-feed and order-book age integrity gates; new signals suspend while data is stale.
- Added distinct stale/recovery alarms.
- Added typed feature snapshot.
- Added baseline confluence signal/risk engine.
- Added JSONL decision/event journal.
- Added terminal and browser audible alarms.
- Added Axum HTTP/WebSocket server and generated Rust→TypeScript contracts.
- Added local React/Vite dashboard and chart signal overlays.
- Added CI workflow for Rust and UI verification.

## 2026-09-22 — Market microstructure checkpoint

### Completed

- Added strongly typed depth dynamics, exact reconstructed L50 metrics, microprice, top-5 imbalance, depth slopes and replenishment/depletion pressure.
- Added rolling trade velocity, signed notional, large-trade pressure, liquidation bursts and OI delta windows.
- Wired these features into live setup scoring and generated contracts.

### Validation

- GitHub Actions run `35708180105` passed the Rust + generated contracts + UI pipeline.

## 2026-09-22 — Typed recorder/replay foundation

### Completed

- Added strongly typed `NormalizedMarketEvent` records for trades, exact book snapshots/deltas, ticker/derivatives, liquidations and klines.
- Added append-only normalized JSONL recorder and typed loader with line-numbered corruption errors.
- Added deterministic exchange-time replay with reset reproducibility tests.

### Validation

- Recorder/replay foundation passed GitHub Actions run `35708641653`.

## 2026-09-22 — Execution simulator + runtime controls

### Completed

- Added validated runtime symbol/timeframe configuration and end-to-end API/dashboard controls with clean symbol state reset/resubscription.
- Added deterministic execution simulator primitives for maker/taker fees, spread/slippage, decision/order latency, partial fills, queue approximation, stop gaps and seeded stress scenarios.
- Preserved the analysis/paper-only boundary; no autonomous real-money execution path exists.

### Validation

- Runtime controls passed GitHub Actions run `35715587022`.

## 2026-09-22 — Outcome labels + leakage-safe research features

### Completed

- Added deterministic MFE/MAE and time-to-target/stop outcome labeling with conservative same-candle stop-first handling.
- Added a Polars research feature factory with causal rolling microstructure/flow features, explicit schema versioning, chronological/embargo splitting and target/outcome-column rejection.
- Added prefix-invariance/no-lookahead tests and CI coverage for the Python research tests.

### Validation

- GitHub Actions run `35725620584` passed for `a1e99e1060671818a20a41c0b850cc670441cc86`, including Rust tests, generated contracts, UI checks and research tests.

## 2026-09-22 — Strict replay feed-integrity checkpoint

### Completed

- Added source-order feed-integrity validation before deterministic replay can sort events by exchange timestamp.
- Strict replay now rejects an order-book delta before a snapshot/reset, non-monotonic Bybit book sequence numbers, and non-monotonic update IDs per symbol.
- Snapshot/update-id reset semantics remain compatible with Bybit reconnect/reset behavior.
- Added tests for valid monotonic snapshot→delta streams and each corruption path.
- Kept the legacy infallible replay constructor for existing callers while adding `DeterministicReplay::try_new` for strict research/evaluation paths.

### Validation

- GitHub Actions run `35731508705` passed for `4ef05968e0b96c056339751fc68c72be169a1302`.

## 2026-09-22 — Bybit typed-record normalization bridge

### Completed

- Added a dedicated Rust normalizer that maps Bybit V5 public websocket payloads into the existing strongly typed `NormalizedMarketEvent` schema.
- Preserves exchange timestamps, exact L50 snapshot/delta update IDs and sequence IDs, including zero-size book levels required to replay deletions correctly.
- Covers trade batches, ticker/derivatives updates, liquidations and klines while ignoring heartbeat/control payloads.
- Added unit tests for exact book-delta preservation, batched trade timestamps and control-message rejection.

### Validation

- GitHub Actions run `35738131690` passed for `b5bf43de2a45c692fbb5fa9dff055045e174b37b`.

## 2026-09-22 — Live recorder integrity admission gate

### Completed

- Added `RecordingIntegrityGate` and `AcceptedMarketRecorder` in the Rust engine.
- The live-recorder facade rejects L50 deltas before a snapshot and stale/non-monotonic sequence or update IDs before any disk write can occur.
- Snapshot/update-id reset semantics allow a clean Bybit reconnect without carrying the old cursor forward.
- Added tests for delta-before-snapshot rejection, monotonic snapshot/delta acceptance, stale sequence/update rejection and reconnect reset behavior.

### Validation

- GitHub Actions run `35745161038` passed for `095029b25459a5421f56d2f975c97b5c59aafe85`.

## 2026-09-22 — Atomic Bybit recorder batch checkpoint

### Completed

- Added configurable `A6_MARKET_RECORD_PATH` with a safe local default at `data/market-events.jsonl`.
- Added `AcceptedMarketRecorder::append_bybit_message`, joining the tested Bybit V5 normalizer directly to the integrity-gated typed recorder API.
- Whole websocket payloads are integrity-validated against a cloned cursor before any event in that payload is written, preventing partial batch persistence and preventing rejected batches from advancing the live L50 cursor.
- Control/heartbeat payloads remain zero-write operations.

### Validation

- Baseline Actions run `35745161038` is green.
- Code commits: `f8be4d6edc1ce07b4b7e954cb2e3a1082c509381` and `359e231661299f771a4a79c0dfc0b5a0e0e7c197`.
- New-head CI is pending; do not claim this checkpoint green until it completes.

### Current highest-priority gaps

1. Invoke `append_bybit_message` from the production live websocket path only after the corresponding market-state payload is accepted; rejected stale/non-monotonic L50 messages must remain zero-write.
2. Feed strict deterministic replay through the exact production Rust market-state → feature → signal path.
3. Add Parquet/DuckDB typed persistence.
4. Drive the execution simulator from replay for end-to-end outcome evaluation.
5. Continue the remaining research/ML calibration and champion/challenger roadmap only after the replay path is trustworthy.
