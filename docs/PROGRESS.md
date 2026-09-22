# Progress Log

## 2026-09-22 — Bootstrap

### Completed

- Initialized empty `agent-6` repository and local-only architecture/setup.
- Added Rust event-driven core, Bybit V5 live linear-perpetual gateway, REST backfill, 1m/3m/5m/15m state, exact L50 reconstruction, trades/ticker/liquidations, typed features, baseline signal/risk engine, JSONL journal, audible alarms, Axum API, generated Rust→TypeScript contracts, React/Vite dashboard and CI.

## 2026-09-22 — Market microstructure checkpoint

- Added exact reconstructed L50 metrics, microprice, top-5 imbalance, depth slopes/replenishment/depletion pressure, rolling trade velocity/signed notional/large-trade pressure, liquidation bursts and OI delta windows.
- Validation: Actions `35708180105` passed.

## 2026-09-22 — Typed recorder/replay foundation

- Added strongly typed normalized trade/book/ticker/liquidation/kline events, append-only JSONL recorder, typed corruption-aware loader and deterministic exchange-time replay.
- Validation: Actions `35708641653` passed.

## 2026-09-22 — Execution simulator + runtime controls

- Added validated runtime symbol/timeframe controls and deterministic simulator primitives for fees, spread/slippage, latency, partial fills, queue approximation, stop gaps and seeded stress scenarios.
- Real-money autonomous execution remains absent/disabled.
- Validation: Actions `35715587022` passed.

## 2026-09-22 — Outcome labels + leakage-safe research features

- Added deterministic MFE/MAE/time-to-target-stop labels, Polars causal feature factory, schema versioning, chronological/embargo splitting, target-column rejection and prefix-invariance/no-lookahead tests.
- Validation: Actions `35725620584` passed.

## 2026-09-22 — Strict replay feed-integrity checkpoint

- Added source-order integrity validation before replay sorting; strict replay rejects delta-before-snapshot and non-monotonic Bybit seq/update IDs while supporting snapshot/reset semantics.
- Validation: Actions `35731508705` passed.

## 2026-09-22 — Bybit typed-record normalization bridge

- Added Bybit V5 → `NormalizedMarketEvent` mapping preserving exchange timestamps, exact L50 IDs and zero-size deletion levels; covers trade batches, ticker, liquidations and klines while ignoring control payloads.
- Validation: Actions `35738131690` passed.

## 2026-09-22 — Live recorder integrity admission gate

- Added `RecordingIntegrityGate` and `AcceptedMarketRecorder`; stale/non-monotonic L50 data is rejected before disk I/O and reconnect snapshots reset the cursor.
- Validation: Actions `35745161038` passed.

## 2026-09-22 — Atomic Bybit recorder batch checkpoint

- Added `A6_MARKET_RECORD_PATH` and atomic `AcceptedMarketRecorder::append_bybit_message`; whole websocket payloads are prevalidated before any write.
- Validation: Actions `35752599990` passed.

## 2026-09-22 — LightGBM baseline + CI dependency repair

- Added leakage-conscious LightGBM TP-before-SL meta-label baseline with chronological validation, embargo and AUC/Brier/log-loss metrics.
- Repaired CI to install the research package from `research/pyproject.toml` plus Pytest and cache dependencies from that manifest.
- Failed run `35759002809` was isolated to missing Python research dependencies; Rust/contracts/UI were green.
- Fix commit: `9cf3a5ac30d47a71910cc0073df939c8fc1cc690`.
- Validation: Actions `35765555560` passed for `e37e4b6bdf1fa49996ef3dbb594992d4367b29d3`, confirming the dependency repair is green.

## 2026-09-23 — Probability calibration + NO_TRADE checkpoint

### Completed

- Added held-out Platt and isotonic probability calibrators without refitting or inspecting feature data.
- Added Brier score and expected calibration error (ECE) reporting.
- Added validation-only `NO_TRADE` threshold selection using realized utility across all opportunities, with explicit minimum coverage so tiny cherry-picked subsets cannot win by conditional accuracy alone.
- Added strict probability/label validation and tests for bounded calibration, isotonic monotonicity, perfect-calibration ECE, abstention selection and malformed inputs.
- Updated the 24-hour roadmap to mark calibration, Brier/ECE and validation-derived NO_TRADE threshold complete.

### Validation

- Previous head Actions run `35765555560` is confirmed green.
- Calibration commits: `4f5208aab534c587fc557bfb2a77e8986101db00`, `2e1e61065ef52c8a0b1a6c67e8e4f5c42845427a`.
- Calibration head Actions run `35772137230` passed.

## 2026-09-23 — Probability drift monitor checkpoint

### Completed

- Added deterministic probability Population Stability Index (PSI) against a frozen reference distribution using fixed [0,1] bins.
- Added mean-confidence and NO_TRADE/abstention-rate shift guardrails with explicit thresholds.
- Added strict finite/range validation and tests for stable windows, severe confidence drift and malformed inputs.
- Monitoring is diagnostic only: it does not retrain, promote a model or enable trade execution.

### Validation

- Previous head Actions run `35772137230` is confirmed green.
- Drift code commit: `1c2bde69918f8d0444192a57bac4e412aad5a657`; tests: `4fba44ca16b9e8c2c1a32f71b67e4f3a6173d159`.
- Drift head Actions run `35778894627` passed.

## 2026-09-23 — Confidence bucket diagnostics checkpoint

### Completed

- Added deterministic equal-width held-out confidence bucket reporting with count, coverage, mean probability, observed positive rate and per-bucket Brier score.
- Added strict probability/label validation and boundary coverage including probabilities exactly 0 and 1.
- Added tests proving every observation is accounted for, bucket metrics use only bucket members, and malformed inputs fail closed.
- Reconciled stale roadmap boxes for the already-completed LightGBM baseline, TP-before-SL/time-to-event labels, and this confidence report.
- Reporting remains research-only and cannot enable real-money execution.

### Validation

- Previous head Actions run `35778894627` is confirmed green.
- Confidence code commit: `bb81a6e9513db9941bb2f462d52802db1c305db4`; tests/follow-up: `df91ce46b60b29aef20e219e990e9cd78446e401`, `4c434b789a279f8c8762aedc0a5d841a725e739c`.
- Confidence head Actions run `35785267026` passed.

## 2026-09-23 — Purged chronological split checkpoint

### Completed

- Added outcome-horizon-aware purged chronological train/validation splitting for research.
- Training observations whose `label_end_ms` overlaps the validation era are removed before fitting; an optional row embargo excludes the immediate boundary as a second leakage guard.
- Added strict checks for monotonic event time, impossible label horizons, malformed arrays, and the fail-closed case where purge/embargo removes all training data.
- Added tests proving overlap purge and embargo behavior and marked the roadmap item complete.

### Validation

- Previous head Actions run `35785267026` is confirmed green.
- Code commit: `0ff3139620fe282b47f0146fbfc0f522bf562471`; tests: `37f69b7e1d514aa3d25bfe70d5853353cec47845`.
- Actions run `35791057802` passed for `d82a154934b0d0c3586faf0031e3e8335451ef19`.

## 2026-09-23 — Model artifact integrity checkpoint

### Completed

- Added deterministic content-addressed `ModelArtifactManifest` metadata for offline challenger artifacts, including model family, feature schema, training-data identity, code revision, metrics and parameters.
- Added SHA-256 hashing of candidate bytes plus canonical manifest hashing and fail-closed artifact verification so tampered model bytes cannot silently match a recorded candidate.
- Added strict validation for required provenance fields and finite metrics, with tests for deterministic manifests, successful verification and tamper rejection.
- This is research-only metadata; it does not promote a model or enable real-money execution.

### Validation

- Previous head Actions run `35791057802` is confirmed green.
- Artifact code commit: `13080c632982f8180b49947e3e23ff14d44a4829`; tests: `626af434b8ad0f33be27408de56e73d16eef969c`.
- New-head CI pending; do not claim this checkpoint green until Actions completes.

### Current highest-priority gaps

1. Invoke `append_bybit_message` from the production accepted websocket path; rejected stale/non-monotonic L50 payloads must remain zero-write.
2. Feed strict deterministic replay through the exact production Rust market-state → feature → signal path.
3. Add Parquet/DuckDB typed persistence.
4. Drive the execution simulator from replay for end-to-end outcome evaluation.
5. Add CatBoost challenger work, then champion/challenger registry and rollback.
