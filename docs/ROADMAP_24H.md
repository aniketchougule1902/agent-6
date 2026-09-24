# Agent-6 — 24-Hour Build Roadmap

This is the hourly build queue, not an autonomous running schedule. At each checkpoint inspect the repository and CI, implement the highest-impact incomplete item, run the relevant verification, and append evidence to `docs/PROGRESS.md`. The checkpoints are targets; a complete and validated system cannot be guaranteed by elapsed time alone.

`python scripts/hourly-check.py` provides hourly automated validation for 24 hours and logs failures. It does not implement the unchecked work automatically.

## Hourly checkpoint queue

| Hour | Implementation checkpoint | Evidence to record |
| --- | --- | --- |
| H0 | Inspect CI, live feed and recorder admission | Test output and live health |
| H1 | Close replay-to-production-state gap | Replay/live parity fixture |
| H2 | Persist normalized events to typed analytical storage | Round-trip and schema check |
| H3 | Drive simulator from replay | Deterministic fill and outcome report |
| H4 | Add research multi-timeframe features | Prefix-invariance tests |
| H5 | Add derivatives features | Causal feature tests |
| H6 | Add bounded Optuna search | Held-out comparison |
| H7 | Define strategy/model genome | Schema and validation tests |
| H8 | Build challenger generation | Reproducible candidates |
| H9 | Automate replay evaluation | After-cost report |
| H10 | Add champion registry | Artifact integrity check |
| H11 | Add rollback and critic records | Rollback test |
| H12 | Add a second venue adapter | Recorded feed fixture |
| H13 | Normalize venue clocks | Timestamp ordering tests |
| H14 | Add lead/lag and divergence features | No-lookahead tests |
| H15 | Isolate venue outages | Fault-injection check |
| H16 | Wire paper equity to circuit breaker | Loss/drawdown tests |
| H17 | Add exposure and duplicate-signal controls | Risk tests |
| H18 | Add volatility/model/data halts | Halt/recovery tests |
| H19 | Build Windows launcher and preflight | Fresh clone smoke test |
| H20 | Build Linux/macOS launcher | Clean environment smoke test |
| H21 | Add graceful shutdown and health details | Reconnect/termination test |
| H22 | Run full replay, cost and calibration evaluation | Out-of-sample report |
| H23 | Complete docs and paper shadow checklist | Final test and limitations log |

Each hour begins with the highest-impact incomplete checkpoint. Failed tests take priority over the next row. Unfinished rows stay open; elapsed time does not mark them complete.

## H0–H2 — Foundation ✅

- [x] Initialize repository and local-only README
- [x] Rust workspace and strict typed market structures
- [x] Bybit V5 linear public WS connector
- [x] 1m/3m/5m/15m REST backfill
- [x] WebSocket heartbeat/reconnect loop
- [x] L50 order-book, trades, ticker, liquidation parsing
- [x] JSONL decision/event journal
- [x] Rust-to-TypeScript type generation

## H2–H4 — Live vertical slice ✅

- [x] Baseline multi-factor signal engine
- [x] Entry / SL / TP1 / TP2 lifecycle tracking
- [x] Local WebSocket API
- [x] TradingView Lightweight Charts dashboard
- [x] Audible browser alarms + terminal fallback
- [x] CI green on Rust + UI
- [x] Add stale-feed watchdog and sequence-age metrics

## H4–H6 — Market-state depth

- [x] Correct L50 snapshot/delta local book reconstruction rather than message-only imbalance
- [x] Add microprice and top-N imbalance
- [x] Add book slope, depletion and replenishment
- [x] Trade velocity, signed notional and large-trade detection
- [x] Liquidation burst features
- [x] Rolling OI delta windows
- [x] Runtime symbol/timeframe controls without restart

## H6–H8 — Persistence and replay

- [x] Typed event store using Parquet/DuckDB
- [x] Raw normalized market-event recorder
- [x] Connect recorder admission to the production live websocket path
- [x] Deterministic replay clock
- [x] Capture validated REST bootstrap klines in the normalized session recording
- [x] Re-run live Rust feature/signal code against replay
- [x] MFE/MAE and time-to-target outcome labeling

## H8–H10 — Realistic simulator

- [x] Maker/taker fee model
- [x] spread + slippage model
- [x] order latency and decision latency
- [x] stop gaps and partial fills
- [x] queue/fill approximation
- [x] deterministic seeded stress scenarios
- [ ] Drive simulator from normalized production replay and emit after-cost outcome report

## H10–H12 — Research feature factory

- [x] Polars training dataset builder
- [x] regime labels
- [x] multi-timeframe features
- [x] microstructure features
- [x] derivatives features
- [x] no-lookahead validation checks
- [x] feature-schema versioning

## H12–H14 — ML baseline

- [x] LightGBM meta-label model
- [x] TP-before-SL label
- [x] time-to-event labels
- [x] CatBoost challenger baseline
- [x] purged chronological splits
- [ ] Optuna research-only tuning
- [x] model artifact manifest

## H14–H16 — Confidence and abstention

- [x] isotonic / Platt calibration
- [x] Brier score + ECE metrics
- [x] uncertainty/disagreement features
- [x] NO_TRADE threshold from validation
- [x] confidence bucket report
- [x] probability drift monitor

## H16–H18 — Self-evolution

- [x] Promotion policy skeleton
- [ ] strategy/model genome format
- [ ] challenger generator
- [ ] automated replay evaluation
- [ ] champion registry
- [ ] signed/hashed artifacts
- [ ] rollback path
- [ ] critic post-trade diagnosis records

## H18–H20 — Multi-venue context

- [ ] Binance market adapter
- [ ] OKX market adapter
- [ ] venue clock normalization
- [ ] lead/lag features
- [ ] cross-venue spread and flow divergence
- [ ] exchange-outage isolation

## H20–H22 — Risk hardening

- [ ] max daily loss/drawdown circuit breakers
- [ ] risk budget and exposure state
- [ ] repeated-signal dedupe
- [ ] abnormal-volatility halt
- [ ] stale model halt
- [ ] corrupted/missing data halt

## H22–H24 — Production-local finish

- [ ] Windows one-command launcher
- [ ] Linux/macOS launcher
- [ ] startup preflight
- [ ] graceful shutdown
- [ ] local health dashboard
- [ ] full test suite
- [ ] README final verification
- [ ] architecture/progress docs final verification
- [ ] paper/shadow run checklist

## Definition of complete

“Complete” means the local analysis/paper system boots from documented commands, receives live data, produces/abstains from typed signals, renders chart overlays, rings on signal/TP/SL events, journals decisions, can replay historical events, trains/evaluates challengers without leakage, calibrates confidence, and can promote/rollback a champion only through validation gates.

It does **not** mean that any particular return or win rate is guaranteed.
