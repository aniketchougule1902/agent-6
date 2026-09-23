# Progress Log

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

- Added held-out Platt and isotonic calibration, Brier score, ECE, validation-derived `NO_TRADE` selection with minimum coverage, confidence-bucket diagnostics, and deterministic PSI/mean-confidence/abstention-rate drift monitoring.
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

### Current highest-priority gaps

1. Invoke `append_bybit_message` from the production accepted websocket path; rejected stale/non-monotonic L50 payloads must remain zero-write before live state mutation.
2. Feed strict deterministic replay through the exact production Rust market-state → feature → signal path.
3. Add Parquet/DuckDB typed persistence.
4. Drive the execution simulator from replay for end-to-end outcome evaluation.
5. Add multi-timeframe/derivatives research features, then champion/challenger registry and rollback.

Real-money autonomous execution remains absent/disabled.
