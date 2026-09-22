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

- Baseline GitHub Actions run `35731508705` was green before this checkpoint.
- Code commits: `e7ab9cca2e5efd128911a83bb6a1207d10eefc56` and `624efd32ecfa721fa53efa64821695c4b9859d0e`.
- New-head GitHub Actions validation is pending; do not claim this checkpoint green until it completes.

### Current highest-priority gaps

1. Call the tested Bybit normalizer only after the live handler accepts a payload and append those events to `MarketRecorder`; rejected stale/non-monotonic L50 deltas must never enter the recording.
2. Feed strict deterministic replay through the exact production Rust market-state → feature → signal path.
3. Add Parquet/DuckDB typed persistence.
4. Drive the execution simulator from replay for end-to-end outcome evaluation.
5. Continue the remaining research/ML calibration and champion/challenger roadmap only after the replay path is trustworthy.
