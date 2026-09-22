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
- CI for the depth-dynamics checkpoint is running; do not mark the roadmap slope/depletion item complete until the calculations are wired into every L50 update and the run is green.

### Current limitations

- Depth dynamics primitives are implemented and tested but still need to be wired into the L50 recalculation path and exposed in `FeatureSnapshot` before they affect signal scoring.
- Trade velocity/signed notional, large-trade detection, liquidation bursts and rolling OI windows remain incomplete.
- Runtime symbol/timeframe switching remains incomplete.
- The initial confidence number is a heuristic quality score and is intentionally marked uncalibrated.
- The ML/replay/calibration/evolution pipeline is scaffolded but not yet complete.
- No real-money order execution is enabled.

### Next highest-impact work

1. Wire depth slope/replenishment/depletion into exact L50 updates, typed feature snapshots and bounded signal scoring; keep it neutral until sufficient live depth exists.
2. Add trade velocity, signed notional, large-trade and liquidation-burst features.
3. Add rolling OI delta windows.
4. Add runtime symbol/timeframe controls.
5. Implement typed recorder/replay.
