# Agent-6 — 24-Hour Build Roadmap

This is the working queue for the hourly build automation. Each run should inspect the repository and CI first, then implement the highest-impact incomplete item without duplicating completed work.

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
- [ ] Runtime symbol/timeframe controls without restart

## H6–H8 — Persistence and replay

- [ ] Typed event store using Parquet/DuckDB
- [ ] Raw normalized market-event recorder
- [ ] Deterministic replay clock
- [ ] Re-run live Rust feature/signal code against replay
- [ ] MFE/MAE and time-to-target outcome labeling

## H8–H10 — Realistic simulator

- [ ] Maker/taker fee model
- [ ] spread + slippage model
- [ ] order latency and decision latency
- [ ] stop gaps and partial fills
- [ ] queue/fill approximation
- [ ] deterministic seeded stress scenarios

## H10–H12 — Research feature factory

- [ ] Polars training dataset builder
- [ ] regime labels
- [ ] multi-timeframe features
- [ ] microstructure features
- [ ] derivatives features
- [ ] no-lookahead validation checks
- [ ] feature-schema versioning

## H12–H14 — ML baseline

- [ ] LightGBM/CatBoost meta-label model
- [ ] TP-before-SL label
- [ ] time-to-event labels
- [ ] purged chronological splits
- [ ] Optuna research-only tuning
- [ ] model artifact manifest

## H14–H16 — Confidence and abstention

- [ ] isotonic / Platt calibration
- [ ] Brier score + ECE metrics
- [ ] uncertainty/disagreement features
- [ ] NO_TRADE threshold from validation
- [ ] confidence bucket report
- [ ] probability drift monitor

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
