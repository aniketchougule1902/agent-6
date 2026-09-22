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

### Current limitations

- Book depletion/replenishment, slope and queue dynamics are not yet modeled.
- The initial confidence number is a heuristic quality score and is intentionally marked uncalibrated.
- The ML/replay/calibration/evolution pipeline is scaffolded but not yet complete.
- No real-money order execution is enabled.

### Next highest-impact work

1. Add depth slope, replenishment/depletion, trade velocity and liquidation-burst features.
2. Add runtime symbol/timeframe controls.
3. Implement typed recorder/replay.
4. Build realistic execution/cost simulator.
5. Build the calibrated meta-label research pipeline.
