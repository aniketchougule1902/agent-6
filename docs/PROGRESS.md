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
- Added entry/SL/TP1/TP2 lifecycle tracking.
- Added JSONL decision/event journal.
- Added terminal bell for critical alerts.
- Added Axum HTTP/WebSocket server.
- Added `ts-rs` Rust→TypeScript contract generation.
- Added local React/Vite dashboard using TradingView Lightweight Charts.
- Added chart entry/SL/TP overlays.
- Added distinct WebAudio alarm patterns for signal, TP, SL and feed state.
- Added research promotion-policy skeleton.
- Added CI workflow for Rust and UI verification.
- First full GitHub Actions run passed Rust tests, generated contracts, UI typecheck and UI build.

## 2026-09-22 — Market microstructure checkpoint

### Completed

- Added a dedicated, strongly typed Rust `microstructure` module for depth dynamics.
- Added normalized near-touch/deep-book slope calculation that is comparable across price levels.
- Added update-to-update bid replenishment / ask depletion pressure calculation with bounded output.
- Added unit tests for near-touch slope direction, bid-replenishment pressure, and empty-book neutrality.
- Extended internal market state with bid/ask depth slope, previous-side depth, and depth-pressure fields so the exact L50 reconstruction can feed these metrics without changing the external API prematurely.

### Validation

- Previous `main` CI run `35700607804` passed the complete Rust + generated TypeScript + UI pipeline.

## 2026-09-22 — Live flow + derivatives depth checkpoint

### Completed

- Wired the tested depth-dynamics module into every accepted reconstructed L50 book update.
- Snapshot/reset paths clear prior-depth baselines so reconnects do not create synthetic replenishment spikes.
- Exposed bid/ask depth slope and bounded replenishment/depletion pressure through the generated Rust→TypeScript feature contract.
- Added 5-second trade velocity and signed quote-notional tracking, rolling large-trade imbalance, signed liquidation-notional tracking and a 5-second liquidation-burst feature.
- Added bounded rolling OI sample buffers plus 1-minute and 5-minute OI delta features.
- Integrated the new microstructure features into bounded live setup scoring and dashboard metrics.
- Added unit coverage for large-trade pressure, directional liquidation bursts and OI-window deltas.

### Validation

- GitHub Actions run `35708180105` passed for code commit `ea33824219564aad20b8c6dcecb56d5b33401d1e`.
- Rust tests, Rust→TypeScript contract generation, UI typecheck, and UI production build all completed successfully.

## 2026-09-22 — Typed recorder/replay foundation

### Completed

- Added a strongly typed `NormalizedMarketEvent` schema covering trades, exact order-book snapshots/deltas, ticker/derivatives state, liquidations and klines.
- Added append-only normalized JSONL `MarketRecorder` primitives with explicit error context and directory creation.
- Added typed recording loader with line-numbered decode failures rather than silently accepting corrupt replay data.
- Added a deterministic replay iterator that sorts by exchange timestamp, exposes replay-relative elapsed time and can reset to reproduce the identical event sequence.
- Added unit tests for deterministic ordering/time, reset reproducibility and typed JSON round-tripping.
- Compiled the replay module into the Rust engine so CI exercises the new tests.

### Validation

- The recorder/replay foundation subsequently passed on `main` in GitHub Actions run `35708641653`.

### Current limitations

- Recorder primitives are not yet wired to every accepted live normalized Bybit event.
- Replay does not yet feed the exact live state/feature/signal path.
- Parquet/DuckDB persistence and MFE/MAE outcome labeling remain incomplete.

## 2026-09-22 — Execution simulator + runtime-control foundation

### Completed

- Added validated, strongly typed runtime market configuration primitives for symbol/timeframe changes, including a monotonically increasing generation for safe reconnect coordination.
- Runtime values reject malformed symbols and unsupported timeframes before they can reach a market subscription.
- Added a strongly typed deterministic execution simulator with explicit maker/taker fee schedules.
- Added spread-aware and square-root book-impact slippage, separate decision/order latency, touch-liquidity capacity, maker queue-ahead approximation and partial fills.
- Stop simulations can use a worse gap-price baseline instead of assuming a fill at the stop trigger.
- Added deterministic seeded stress configurations that increase latency/slippage and reduce fill capacity reproducibly.
- Added unit coverage for fees/slippage/latency/partial fills, stop gaps, maker queue effects and reproducible stress scenarios.
- Preserved the analysis/paper-only boundary; no autonomous real-money execution path was added.

### Validation

- The previous `main` head was green in GitHub Actions run `35708641653` before this checkpoint.
- GitHub Actions run `35714388993` is validating compiled runtime-control and simulator modules; do not claim this checkpoint green until it completes.

### Current limitations

- Runtime control primitives are initialized and typed but are not yet exposed through the local API/dashboard or wired to force a clean Bybit resubscription/state reset; the roadmap runtime-control box therefore remains open.
- The simulator is a tested execution-model primitive but is not yet driven by deterministic replay/trade outcome evaluation; simulator roadmap boxes remain open until that integration exists.
- Raw recorder wiring and end-to-end replay remain higher priority than research/ML work.
- Real-money autonomous execution remains disabled.

### Next highest-impact work

1. Finish runtime symbol/timeframe controls end-to-end: local API, dashboard control, clean feed resubscription and market-state reset.
2. Wire every accepted normalized Bybit event into the typed recorder.
3. Feed deterministic replay through the exact production Rust state/feature/signal path.
4. Drive the execution simulator from replay and add MFE/MAE/time-to-target labels.
5. Add Parquet/DuckDB persistence and the no-lookahead research feature factory.
