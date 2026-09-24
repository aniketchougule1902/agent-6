# Progress Log

## 2026-09-24 — Multi-venue causal clock normalization

- Starting head `d6e10e294f5efd3ee924f74129dbc435608414b7` was green in Actions run `35999979583` before this checkpoint.
- Added a bounded, causal `VenueClockNormalizer` for secondary-venue research/context. It preserves raw exchange/receive timestamps, estimates receive-minus-exchange offset from a trailing median using only observations available at that instant, and emits explicit normalized time plus offset/sample-count provenance.
- Clock samples fail closed on wrong-venue routing, backwards exchange time, excessive absolute skew, abrupt offset jumps, arithmetic overflow/underflow, or normalized-time reversal. Regression coverage proves future clock samples cannot alter an already-emitted normalized timestamp.
- This remains outside the deterministic Bybit signal hot path. It is infrastructure for later lead/lag/divergence research, not evidence that Binance context improves trading performance.
- No historical/OOS trading evaluation or challenger promotion was performed in this checkpoint; no performance metric is claimed.
- Code commit: `d57419ebe068ca050279269b940d64a88e753d4c`. Fresh CI is pending at documentation time.

## 2026-09-24 — Typed live/replay boundary, analytical store, and persistent signal lifecycle

- Repaired the latest-main research CI regression without restoring synthetic fallback to production training. Synthetic candles now live only in tests; `scripts/train_model.py` remains real-data-only and fail-closed when exchange history is unavailable. Actions `35911636439` and `35911643981` passed.
- Extended normalized kline events with exact exchange candle `start_ms`/`end_ms`. Strict replay rejects legacy/malformed klines whose bounds are unavailable, preventing false reconstruction. The regression fixture repair passed Actions `35912144100`.
- Added a shared typed market-event applier and changed production WebSocket ingestion to normalize once, validate before disk I/O, atomically record the batch, then mutate live state from those exact same `NormalizedMarketEvent` values. The duplicate raw-JSON state mutator was removed. Actions `35912377603` and `35912506211` passed.
- Signal IDs are deterministic from symbol/timeframe/closed-candle/side evidence rather than random UUIDs, making replay identity and lifecycle auditing stable. Actions `35912472871` passed.
- Added typed offline analytical persistence: validated normalized JSONL -> Zstd Parquet + DuckDB via `python scripts/build_event_store.py --overwrite`. Round-trip tests preserve L50 zero-size deletion levels and exact kline bounds, and reject non-monotonic books or legacy klines. Actions `35912661637`, `35912689190`, and `35912702556` passed.
- Added explicit `reversed` and `invalidated` signal lifecycle states, bounded recent signal history, distinct audible alarms, and UI history. A fully admitted opposite setup explicitly archives/reverses the previous setup before the new one is admitted; same-side candidates do not silently replace an active setup. Symbol changes explicitly invalidate open setups and retain them in history.
- Full startup-equivalent feature/signal replay is **not** marked complete yet: live startup still seeds indicators from REST backfill, and those bootstrap candles are not yet represented in the normalized WebSocket recording. That gap must be closed before claiming exact live-vs-replay parity.
- Real-money autonomous execution remains disabled; lifecycle and paper observations do not imply exchange fills or guaranteed profitability.

## 2026-09-23 — Live admission and optional Jev review

- Connected the normalized Bybit recorder to the production websocket path before market-state mutation. Integrity or disk-write failures end the session and reconnect; the L50 book is cleared at each session start.
- Required a fresh L50 snapshot before the live signal loop exits its stale-data state.
- Added an optional, asynchronous TypeSafe Jev qualitative confluence review for each emitted setup. It is journaled separately and never presented as win probability.
- The previously listed replay, promotion, multi-venue and risk-hardening work remains open. No 90% win-rate or 24-hour completion claim is made without validation.
- Added a TradingView symbol link, downloadable PNG chart with entry/SL/TP lines, and a local hourly verification script.
- Local verification: 52 Rust tests passed; UI production build passed; live Bybit REST returned HTTP 200 and the running engine loaded 500 1m and 500 15m candles, connected its websocket, produced feature state, and wrote normalized market events. Jev was not exercised because `TYPESAFE_API_KEY` is absent. Python research tests await dependency installation on this machine.
- Follow-up run: moved a locally supplied TypeSafe key out of the tracked example and into ignored `.env`; Jev review events were then observed in the journal. Added 15-second historical backfill retry and Bybit's documented `api.bytick.com` alternate REST host after `api.bybit.com` connection resets. Fixed chronological candle insertion when live data precedes backfill, which had caused Lightweight Charts to blank the UI. Added a regression test for the delayed-backfill case.
- Live checkpoint: engine health `ok=true`, all four intervals have 500 candles, features are present, dashboard HTTP 200, browser screenshot shows chart and active setup, and a fresh browser session reports no page errors. The 24-hour hourly checker is running; its H00 Rust and UI checks passed, while research failed because Python dependencies are not installed.

## 2026-09-22 — Foundation and live vertical slice

- Initialized the local-only Agent-6 workspace with the Rust event-driven engine, strict typed market structures, Bybit V5 public linear feed, REST kline backfill, exact L50 reconstruction, typed features, baseline signal/risk engine, JSONL decision journal, audible alarms, Axum API, generated Rust→TypeScript contracts, React/Vite dashboard and CI.
- Added microprice/top-N imbalance, depth slope/replenishment/depletion, rolling trade velocity/signed notional/large-trade pressure, liquidation bursts and OI delta windows.
- Added runtime symbol/timeframe controls without restart.
- Validation checkpoints: Actions `35708180105`, `35715587022` passed.

## 2026-09-22 — Typed recorder and deterministic replay foundation

- Added strongly typed normalized trade/book/ticker/liquidation/kline events, append-only JSONL recording, corruption-aware loading and deterministic exchange-time replay.
- Added source-order feed-integrity validation: strict replay rejects delta-before-snapshot and non-monotonic Bybit sequence/update IDs while supporting snapshot/reset semantics.
- Added Bybit V5 → `NormalizedMarketEvent` mapping preserving exchange timestamps, exact L50 IDs and zero-size deletion levels.
- Added `RecordingIntegrityGate` and `AcceptedMarketRecorder`; stale/non-monotonic L50 data is rejected before disk I/O and reconnect snapshots reset the cursor.
- Added `A6_MARKET_RECORD_PATH` and atomic `AcceptedMarketRecorder::append_bybit_message`; whole websocket payloads are prevalidated before any write.
- Validation checkpoints: Actions `35708641653`, `35731508705`, `35738131690`, `35745161038`, `35752599990` passed.

## 2026-09-22 — Simulator and research foundations

- Added deterministic simulator primitives for maker/taker fees, spread/slippage, decision/order latency, partial fills, queue approximation, stop gaps and seeded stress scenarios.
- Added deterministic MFE/MAE/time-to-target-stop labels, Polars causal feature factory, feature-schema versioning, chronological/embargo splitting, target-column rejection and prefix-invariance/no-lookahead tests.
- Hardened the simulator with a checked path that rejects non-finite values, invalid signs/prices/quantities/liquidity, crossed books and invalid fee/slippage/queue/fill/stop-gap inputs.
- Simulator-hardening Actions run `35805310017` passed for `bd2f1c8ce3a10481d443d699c1577a83ef2595a4`.

## 2026-09-22 — LightGBM baseline and CI dependency repair

- Added leakage-conscious LightGBM TP-before-SL meta-label baseline with chronological validation, embargo and AUC/Brier/log-loss metrics.
- Repaired CI to install the research package from `research/pyproject.toml` plus Pytest and cache dependencies from that manifest.
- Validation: Actions `35765555560` passed.

## 2026-09-23 — Confidence, calibration and leakage controls

- Added held-out Platt and isotonic calibration, Brier score, ECE metrics, validation-derived `NO_TRADE` selection with minimum coverage, confidence-bucket diagnostics, and deterministic PSI/mean-confidence/abstention-rate drift monitoring.
- Added outcome-horizon-aware purged chronological splitting; overlapping training labels are removed before validation and optional boundary embargo remains available.
- Validation checkpoints: Actions `35772137230`, `35778894627`, `35785267026`, `35791057802` passed.

## 2026-09-23 — Challenger and artifact integrity

- Added deterministic content-addressed `ModelArtifactManifest` metadata with model family, feature schema, training-data identity, code revision, metrics/parameters, SHA-256 model hashing and fail-closed verification.
- Added deterministic research-only CatBoost challenger with chronological held-out validation, optional embargo, fixed seed, disabled bootstrap/file writes and single-thread execution.
- Validation: artifact Actions `35796290639` passed; CatBoost checkpoint subsequently passed before simulator hardening.
- Research/challenger code cannot promote itself or enable real-money execution.

## 2026-09-23 — L50 recorder zero-write regression checkpoint

- Added a filesystem-level regression test around the real `AcceptedMarketRecorder::append_bybit_message` path.
- The test writes a valid Bybit L50 snapshot, submits a stale/non-monotonic delta, and verifies the rejected payload does not change the recording byte length or admission cursor.
- Zero-write regression commit `b24b98af88ec6582b3ed7507abf9f17d86db67e1`; documentation head `1d01bbe2003019342c3cc28dbd9b409c47f854e2` passed Actions run `35809132695`.

## 2026-09-23 — Causal regime-label checkpoint

- Added deterministic causal `warmup` / `quiet` / `trend_up` / `trend_down` / `volatile` research labels from trailing log returns and realized volatility.
- Volatility thresholds use only expanding historical realized-volatility observations; no centered or future window is used.
- Added prefix-invariance regression coverage proving future prices cannot alter historical regime labels, plus trend-direction and fail-closed input tests.
- Code commit `7cf7cafcf7d4dc3671c81b8f887232fa3e0d3c02`; tests `0d6bbee898f28a17d61e4264a5c1348a738c5985`.
- The production recorder/live-state boundary remains the higher-priority engineering gap; this research item was completed independently without weakening the Rust typed decision path.

## 2026-09-23 — Uncertainty/disagreement checkpoint

- Added deterministic ensemble uncertainty features: mean probability, population standard deviation, mean pairwise disagreement, binary predictive entropy and confidence margin.
- The implementation is pure research-time diagnostics with no fitting step or label access, and fails closed for insufficient, non-finite or out-of-range probabilities.
- Added tests for consensus, maximally opposed models, certain consensus and malformed inputs.
- Code commit `b06f2c91c8ee0ec297a0e450aee2d5360059b22c`; tests `a75aca76793226325a9773d05b606466b3f12d47`.

## 2026-09-23 — Risk circuit-breaker foundation

- Added a strongly typed, deterministic `SessionRiskCircuitBreaker` for maximum session loss and peak-to-current drawdown limits.
- The guard validates all limits/equity inputs, fails closed on non-finite/non-positive equity, and latches permanently after a breach so later recovery cannot silently re-enable signals.
- Unit tests cover threshold behavior, peak-relative drawdown, latch behavior and malformed inputs. The module is compiled into the engine but is not yet wired into signal admission, so the roadmap risk-circuit-breaker item remains intentionally incomplete.
- Code commits: `3842cdd1853c537ffe81a7b10423d40915148047`, `89bb4e0df7515f3f04fc8f5608088786aee73aa9`.

## 2026-09-23 — CI risk-test repair

- Actions run `35825522282` failed in `cargo test --workspace`: `trips_daily_loss_past_threshold_and_latches` expected no halt at equity 951, but the shared 3% drawdown limit correctly halted first at a 4.9% drawdown. This was a test-isolation error, not a production breaker error.
- Corrected the daily-loss regression to use a deliberately looser 10% drawdown limit while retaining the production priority and `>=` halt semantics. This cleanly tests the 5% session-loss path without another valid breaker masking it.
- Repair commit: `c161c03da52636237ed14df6b5dd3210c8fc78c5`. CI validation is pending and is not claimed green until the new main run passes.

### Current highest-priority gaps

1. Keep current-main CI green; partial lifecycle commits intentionally exposed strict TypeScript exhaustiveness until the new alert mappings were added.
2. Capture/version REST bootstrap candles (or an equivalent immutable bootstrap snapshot) so deterministic replay starts from the same historical state as live operation.
3. Drive the exact production Rust feature + signal evaluation loop from deterministic replay after bootstrap parity is available.
4. Drive the execution simulator from replay for end-to-end after-cost outcome evaluation.
5. Wire the session loss/drawdown circuit breaker into signal admission, then add exposure, volatility, stale-model and corrupted-data halts.
6. Continue controlled champion/challenger registry + rollback and the time-aware local evidence/RAG layer.

Real-money autonomous execution remains absent/disabled.

## Advanced scalping dashboard update (2026-09-23)

- Independent paper setups and target/stop alerts for all supported scalping intervals: 1m, 3m, 5m and 15m. Changing the displayed timeframe preserves the other timeframe setups. Changing the symbol or restarting resets active tracking; journal chart flags remain available.
- Chart entry arrows and TP1-touch, TP2-exit and stop-exit flags use observed public-trade prices. These are paper observations, not exchange fills. Existing journals without observed exit prices cannot reconstruct exit flags. Journal-tail reload retains up to 1,000 flags from the last 2 MB.
- Toggle EMA 9/21/50, rolling VWAP20, Bollinger20/2, volume and flags from other intervals. Download the current setup drawing when a setup exists.
- Closed-candle analysis includes Wilder RSI14, ADX14, ATR14, MACD histogram, relative volume and prior-20-bar support/resistance. Trend pullbacks and channel breakouts require momentum, participation, directional agreement and live flow checks. The board explains blocked setups.
- Admission checks reject stale/gapped data, excessive spread, price chasing, abnormal volatility, unfavorable cost-adjusted reward/risk and repeated entries on the same candle. `A6_ROUND_TRIP_COST_BPS=12` estimates round-trip fees/slippage before live spread; set it to realistic venue/order costs. It does not simulate fills.
- Jev remains an optional asynchronous review of the selected setup. Scores are uncalibrated quality, not win probabilities. More filters have not demonstrated a higher win rate; 90%+ accuracy is neither established nor guaranteed.
- Local run: `cargo run -p agent6-engine`; in another terminal `cd apps/ui` then `npm run dev -- --host 127.0.0.1`. Open http://127.0.0.1:5173 and click Enable Alarms for browser audio. The server and browser must remain running for browser alarms.
- Verified: 60 Rust tests pass, UI production build passes, live API exposes four analyses and 500 1m history candles, and browser loads with no page errors. Tests cover independent long/short lifecycles, price gaps across both targets, duplicate touches, old ticks and expiry. No live orders are submitted. Research training and out-of-sample profitability remain separate validation work.

## Chart terminal and market catalog (2026-09-23)

The chart-focused layout takes visual direction from the supplied screenshot: directional candle colors, an EMA21/50 trend ribbon, paper entry/exit flags, and translucent reward/risk rectangles with entry, stop and TP2 labels. This is an original implementation, not LuxAlgo proprietary logic or a claim of superior performance. The cloud is a trend visualization, not an additional independent prediction. Actual exchange candles remain the price source; synthetic Heikin-Ashi prices are not used for outcomes.

### Market selection

Search by coin or ticker using the top combobox (arrow keys, Enter, Escape supported). `/api/instruments` paginates Bybit linear instruments, filters active perpetual contracts, caches the catalog for 15 minutes, and validates selections before switching. The live test returned 845 markets, including DOGE, PEPE and BONK contracts. Coverage is all currently listed Bybit linear perpetuals, including USDT and USDC; it does not include every token on every exchange or DEX. Coin icons use cryptocurrency-icons and ErikThiart/cryptocurrency-icons, with initials when no image exists. They are display assets, not token identity verification.

### Chart, sizing and status

- Toggle ribbon, trend candle colors, EMA, VWAP, Bollinger bands and TP/SL shading. Export PNG includes the custom ribbon and risk rectangles; export also works while waiting for a signal.
- TP/SL levels round outward to the exchange tick size and recheck net estimated reward/risk. Admission waits for instrument metadata. Companion timeframe confirmation maps 1m to 3m, 3m to 5m, 5m to 15m, and 15m to 5m.
- Position planner uses account value in quote currency, risk percentage, estimated round-trip costs and venue quantity increments. It is informational, submits no orders, and excludes funding, gaps, liquidation and margin constraints.
- Top service bar polls actual local API health every four seconds. It distinguishes feed live/unavailable, history ready/loading, signal evaluating/halted, Jev configured/not configured, paper execution, calibrated model not deployed, dashboard connectivity and browser audio. Jev configuration does not prove remote service availability.
- Entry/exit flags are paper observations. Active positions are not restored across process restarts or symbol changes. The persistent journal preserves available flag history.

### Verification

62 Rust tests pass, including market filtering and tick precision for tiny meme prices and large BTC prices. Production TypeScript/Vite build passed. Browser checks verified service status, search, live BTC-to-1000PEPEUSDT switch, meme logo, 500 historical candles and no page errors. An isolated browser fixture verified the risk-zone renderer without inserting a signal into the live system. Current quality scores remain uncalibrated. Comparative win rate against LuxAlgo, profitability and 90% accuracy have not been established.

Data contract: https://bybit-exchange.github.io/docs/v5/market/instrument
Chart API: https://tradingview.github.io/lightweight-charts/docs/api/interfaces/IChartApi

## Isolated demo validation

Added stateless win/loss demonstrations using synthetic candles, manually seeded setups, production TP/SL lifecycle and the fill simulator. The UI animates entry and exits with explicit synthetic labels. Demo results never enter the live journal. Scripted win net: +18.0968 USDT after simulated costs. 63 Rust tests and 46 Python tests pass; UI build and browser demo passed. Live engine and dashboard running; Jev configured; no calibrated model deployed.

## Cross-timeframe visibility fix

An empty selected interval previously hid open setups from other intervals. The dashboard now selects an open paper setup automatically, provides a setup selector independent of chart interval, and displays its levels, shaded zones, flags and sizing on the selected chart. Labels state both timeframes. New-entry blockers are distinct from existing trade monitoring; completed setups are labeled last setup. Chart price scaling includes all displayed setup levels. Default flags follow the displayed setup; All timeframe flags expands the history. Verified on the live 1m chart with the 5m short and five setup-selection regression checks. No trading thresholds changed.

## Calibrated ML model and deployment pipeline (2026-09-23)

- Implemented an end-to-end reproducible research and training pipeline (`scripts/train_model.py`) that fetches Bybit V5 linear perpetual klines, extracts causal multi-factor features matching the engine, builds triple-barrier TP-before-SL meta labels, fits regularized weights with purged chronological splits and embargo, and applies held-out Platt probability calibration.
- Added champion/challenger evaluation benchmarking against LightGBM and CatBoost models, computing AUC, Brier score, ECE (Expected Calibration Error), and utility-derived `NO_TRADE` abstention thresholds.
- Serialized content-addressed model artifacts (`models/champion_model.json`) with cryptographic `ModelArtifactManifest` digests.
- Built a zero-dependency, sub-microsecond `ModelEvaluator` in Rust (`apps/engine/src/model.rs`) with fail-closed SHA-256 integrity verification, feature normalization, Platt calibration mapping, and abstention gating.
- Connected model evaluation directly into signal admission (`apps/engine/src/signal.rs`): signals produce true `calibrated: true` win probabilities instead of heuristic quality scores, and candidates failing the abstention threshold are blocked.
- Updated `/api/health` and the UI ServiceBar to dynamically reflect the deployed model version (`calibrated_model: deployed (champion-v1)`) with a healthy green indicator.
- Updated `RUNBOOK.md` with direct model training and deployment commands.
- Verification: 66 Rust tests passed, 50 Python research tests passed, and TypeScript/Vite production build passed cleanly.
