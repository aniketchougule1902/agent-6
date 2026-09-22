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


## 2026-09-22 — Live flow + derivatives depth checkpoint

### Completed

- Wired the tested depth-dynamics module into every accepted reconstructed L50 book update.
- Snapshot/reset paths now clear prior-depth baselines so reconnects do not create synthetic replenishment spikes.
- Exposed bid/ask depth slope and bounded replenishment/depletion pressure through the generated Rust→TypeScript feature contract.
- Added 5-second trade velocity and signed quote-notional tracking from the public trade stream.
- Added rolling large-trade imbalance using the current 5-second notional distribution rather than a fixed symbol-specific threshold.
- Added signed liquidation-notional tracking and a 5-second liquidation-burst feature relative to the preceding 55-second local baseline.
- Added a bounded rolling OI sample buffer plus 1-minute and 5-minute OI delta features.
- Integrated depth pressure, large-trade flow, liquidation bursts and rolling OI confirmation into bounded live setup scoring.
- Added dashboard metrics for depth pressure, trade velocity, large-flow imbalance, liquidation bursts and rolling OI deltas.
- Added unit coverage for large-trade pressure, directional liquidation bursts and OI-window deltas.

### Validation

- Parent commit `b636d39d375618cbfdd0632337e68fb0f48c6afb` had green CI before this checkpoint.
- New checkpoint CI must pass Rust tests, generated TypeScript contracts, UI typecheck and UI production build before this slice is considered validated.

### Next highest-impact work

1. Add runtime symbol/timeframe controls without process restart.
2. Implement a normalized typed raw-market recorder and deterministic replay clock.
3. Re-run the live Rust feature/signal path against replayed events.
4. Add MFE/MAE and time-to-target outcome labels.
5. Build the realistic fee/slippage/latency/fill simulator.
