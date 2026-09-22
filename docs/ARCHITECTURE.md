# Agent-6 Architecture

## Objective

Agent-6 is a local, continuously running crypto market-intelligence and scalping research system. The design optimizes for **decision quality under latency and risk constraints**, not for a target win rate.

## Decision layers

The live decision path is layered so a single indicator cannot create a trade:

1. **Feed integrity** — heartbeat age, sequence freshness, reconnect state.
2. **Spread/liquidity gate** — rejects poor execution conditions.
3. **Multi-timeframe structure** — 1m execution with 3m/5m/15m context.
4. **Volatility regime** — range, trend, high-volatility, abnormal.
5. **VWAP / mean-location layer** — avoids chasing structurally poor entries.
6. **Momentum layer** — short-horizon price impulse and decay.
7. **L2 order-book imbalance** — depth asymmetry and later delta/replenishment.
8. **Trade-flow imbalance** — aggressive buyer/seller pressure.
9. **Liquidation pressure** — liquidation impulse and exhaustion context.
10. **Open-interest layer** — distinguishes participation expansion/contraction.
11. **Funding/crowding layer** — penalizes crowded directional conditions.
12. **Cross-timeframe agreement** — disagreement becomes a negative feature.
13. **Future cross-venue layer** — Binance/OKX/Coinbase/Hyperliquid lead/lag.
14. **Future basis layer** — mark/index/perp/spot dislocations.
15. **Future anomaly layer** — feed and market-state outlier detection.
16. **Meta-model layer** — learns whether a detected setup should be taken.
17. **Calibration layer** — converts raw scores to observed out-of-sample probabilities.
18. **Risk/execution layer** — costs, slippage, stop geometry, R:R, fill quality.
19. **Abstention layer** — outputs NO_TRADE when uncertainty/disagreement is high.
20. **Drift layer** — suspends strategies when live behavior departs from validation.

## Fast path

Rust owns anything that must react within milliseconds:

- exchange websocket parsing
- order-book state
- rolling windows
- feature snapshots
- deterministic risk gates
- active-signal state
- TP/SL lifecycle events
- alarm/event transport

LLMs never sit in this path.

## Research path

Python owns expensive asynchronous research:

- feature generation over historical replay
- LightGBM/CatBoost/XGBoost baselines
- calibration
- regime-specific models
- sequence/order-book models
- hyperparameter search
- walk-forward evaluation
- champion/challenger generation
- post-trade diagnosis

The research process may propose a model artifact or strategy definition. It cannot activate it directly.

## Self-evolution

Every decision becomes an episode containing the market state, features, model/champion version, decision, lifecycle, MFE, MAE, costs, and eventual outcome.

Evolution follows:

```text
live champion
   -> research challenger
   -> historical replay
   -> purged walk-forward
   -> unseen holdout
   -> fees/slippage/latency stress
   -> shadow mode
   -> promotion policy
   -> new champion OR discard
```

No single losing trade can mutate production behavior.

## Memory

Four logical memory tiers:

- **working memory:** current market/order-book windows
- **episodic memory:** historical decisions and outcomes
- **semantic memory:** discovered regime/setup statistics
- **model memory:** model artifacts, calibration curves, feature schemas and metrics

Long history is retrieved by similarity/statistics rather than stuffed into an LLM prompt.

## Alerts

Critical events are broadcast from Rust and journaled before display:

- signal created
- TP1
- TP2
- SL
- expiry/invalidation
- feed disconnect/reconnect
- model drift
- champion promotion/rollback

Browser WebAudio provides distinct alarm patterns. The server emits a terminal bell as a fallback.

## Execution stance

Initial releases are signal/paper only. Any future live order path must have:

- exchange-specific auth isolated from research code
- explicit enable flag
- max risk per trade/day
- max daily loss and drawdown circuit breaker
- max leverage
- stale-feed kill switch
- idempotent client order IDs
- position reconciliation
- reduce-only exits
- emergency flatten control
- independent audit journal
