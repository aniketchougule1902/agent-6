# Validation and Promotion Rules

## Non-negotiable rules

1. Never random-split financial time series for final evidence.
2. Never train on the holdout period used for promotion.
3. Never select a challenger on gross PnL while reporting net PnL later.
4. Costs, spread and slippage are included before promotion.
5. A higher win rate cannot compensate for negative expectancy.
6. Live/shadow drift can suspend a champion automatically.
7. Research agents cannot directly modify the live champion pointer.

## Required evaluation

Each candidate must report:

- trade count
- net expectancy in R
- average winner / loser
- profit factor
- max drawdown
- TP-before-SL precision/recall
- Brier score
- expected calibration error
- performance by confidence bucket
- performance by regime
- performance by symbol
- performance by hour/session
- fee/slippage sensitivity
- latency sensitivity
- shadow performance

## Promotion gates

The initial code policy requires, at minimum:

- 500 evaluated trades
- positive after-cost expectancy
- profit factor >= 1.15
- maximum calibration error <= 0.08
- non-negative stressed expectancy
- non-negative shadow expectancy
- expectancy improvement versus champion
- no >15% drawdown degradation versus champion

These are engineering defaults, not promises of profitability; they should be tightened after a meaningful dataset exists.

## Confidence language

Until a model is calibrated on unseen data, the UI must call the number a **quality score**.

Only a calibrated model may label it as an estimated probability, and the UI must retain uncertainty/drift context.
