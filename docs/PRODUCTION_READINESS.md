# Production readiness decision — 2026-09-24

Agent 6 should remain a local paper practice terminal until its predictions and execution assumptions have independent evidence. The live health endpoint inspected during this review reported **0 / 500 qualifying calibration outcomes**, a live feed, and no deployed calibrated model. This is an observation at review time, not a permanent counter.

## Delivered in this review

- Beginner mode is the default. One prominent next-step card distinguishes unavailable data, warming, no setup, price outside entry zone, paper practice, and monitoring/completed setups. Advanced view retains radar, indicators, sizing and order flow.
- Entry, stop and both targets use plain labels. Quality is explicitly distinguished from win probability. Paper account results remain historical observations.
- The dashboard checks snapshot arrival independently of WebSocket connectivity. A frozen dashboard blocks entry after five seconds and triggers one stale-data sound when browser alarms are enabled.
- Target, stop, reversal, invalidation, expiry and feed incidents display a persistent latest-event warning with acknowledgement. The event journal retains earlier events for this browser session. Acknowledgement does not close a position or resolve a feed incident.
- Both manual and automatic UI entry require a fresh active setup. Server entry rejects signals older than 60 seconds, future timestamps, prices outside the entry zone, invalid prices, and invalid target ordering. Existing positions keep their original exit lifecycle. The server uses bid/ask midpoint for zone admission, then simulates execution with spread/slippage; the UI uses last traded price as a preliminary check. The server can reject an apparently eligible UI entry.
- Model deployment rejects invalid thresholds, scales, future timestamps, unsupported calibration methods and nonfinite baseline/sample metrics. These are validity checks, not new evidence of accuracy.

The 60-second admission window is a conservative product rule, not an optimized or profitability-tested strategy parameter. Compare its impact in replay before tuning it. No signal-quality thresholds were increased to create an appearance of accuracy.

## Next work, in priority order

1. **Make outcomes trustworthy.** Record REST bootstrap candles alongside stream events, prove startup/live/replay feature parity, and track every admitted signal across symbol changes and restarts. Attribute missing outcomes explicitly: interrupted feed, market switch, reversal, invalidation, unresolved or completed. Audit why qualifying outcomes are currently zero. Do not silently treat incomplete observations as wins or losses.
2. **Measure economic performance.** Use trade-level replay with partial targets and conservative same-bar stop/target ordering. Report expectancy after fees, spread, slippage and funding; drawdown; coverage; sample counts; and uncertainty intervals by market, timeframe, side and volatility regime. Quote polling currently misses between-poll crossings and is not execution evidence.
3. **Protect the independent test set.** The current five-minute calibration job repeatedly evaluates an evolving chronological test set. Add a versioned dataset ledger, purged walk-forward folds and a final locked holdout plus later shadow period. Require improvement over both the constant-probability and current heuristic baselines. Probability calibration alone must never authorize real-money deployment.
4. **Develop challengers only after labels are sound.** Compare regularized logistic regression with a bounded tree model using the same point-in-time features and costs. Select abstention thresholds on validation data, measure coverage and precision together, then evaluate once on the locked holdout. Avoid targeting an arbitrary 90% win rate; a high win rate can still lose money.
5. **Operational hardening.** Add durable alert replay/cursors, acknowledged incident history, reliable background notification delivery, persisted signal tracking, schema migrations, backup/restore drills, resource limits and multi-day outage/reconnect testing. Browser audio needs a gesture and a running browser; it is not an unattended alarm service.
6. **Real execution is a separate release.** Require authenticated order management, idempotency, exchange reconciliation, reduce-only exits, exchange quantity/margin checks, daily/equity loss limits, exposure caps, a kill switch and tested crash recovery. Remote hosting additionally needs authentication, TLS and durable per-user storage.

## Verification

- Rust workspace: 104 tests pass, including late/chased/invalid paper-entry rejection without account mutation and model deployment checks.
- Research: 102 tests pass.
- UI: TypeScript/Vite production build passes; three guidance regression tests cover both directions, disconnects, frozen snapshots, late entries and terminal setups.
- Browser: live beginner page renders, advanced toggle works, and no browser error logs were observed in the inspected session. At a 390px phone viewport the chart no longer causes horizontal page overflow. The actual engine restart displayed the unavailable-data halt and recovered after reconnection. Alarm enablement was verified; audible delivery under browser suspension was not tested.
- The release engine was rebuilt and restarted with no open paper positions. Health recovered to live/ready and the existing paper account balance and closed-trade history survived.

No improved win rate, profitable strategy, real exchange execution or unattended production readiness is established by these checks.

Risk wording reference: [CFTC virtual currency risk advisory](https://www.cftc.gov/LearnAndProtect/AdvisoriesAndArticles/understand_risks_of_virtual_currency.html). Leverage amplifies risk; the present product submits no real-money orders.
