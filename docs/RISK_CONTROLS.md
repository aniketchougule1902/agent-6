# Paper risk controls

Agent-6 remains a local analysis/paper system. These controls do not enable exchange order execution.

## Entry admission

A new paper entry is rejected unless the selected-market feed is fresh and the referenced signal is still present. Session equity is reconstructed from persisted closed positions plus current marked equity and is checked against a 5% session-loss limit and a 3% peak-to-current drawdown limit.

Paper exposure is also bounded before `Paper::enter` mutates the ledger. Current defaults are:

- maximum aggregate open entry-notional: 50% of current paper equity;
- maximum open entry-notional in one symbol: 25% of current paper equity;
- maximum simultaneously open paper positions: 3.

The exposure evaluator is typed and fail-closed. Non-finite/non-positive equity or requested notional, malformed existing exposure, arithmetic overflow, invalid limits, and empty symbols reject admission rather than falling back to an unsafe value. Existing `Paper::enter` logic separately prevents entering the same stable signal ID twice and requires enough available paper balance.

Open exposure is measured from each position's immutable entry price times remaining quantity. This is intentionally conservative and deterministic for admission; marked equity remains the denominator so losses tighten the budget.

## Session breaker persistence status

`RiskStateSnapshot` is versioned and validates the session start, observed peak, latest equity, limits, timestamp, and latched halt reason. A latched breaker never clears merely because equity recovers.

The snapshot format is ready, but crash-safe transactional persistence of the intratrade peak/latch alongside the paper ledger is still incomplete. Until that wiring is finished, startup reconstruction from closed-equity history cannot recover a drawdown that breached and fully recovered before a crash without leaving a closed-equity trace. Do not treat this limitation as solved.

## Alerts and next work

Existing signal/TP/SL/feed alarms remain unchanged. A dedicated risk-halt alarm/UI state should be emitted when the durable breaker persistence is wired, without blocking monitoring or exits for positions that were already open.
