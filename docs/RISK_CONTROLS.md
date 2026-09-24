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

A new `RiskStateStore` provides an atomic, fsync-backed sidecar format bound to the paper account revision. It validates the nested risk snapshot and fails closed on corrupt JSON, unknown schema versions, unsafe/unlatched breached snapshots, or a paper/risk revision mismatch. Successful replacement also fsyncs the containing directory where supported. Tests cover durable latch round-trip, revision mismatch, corruption/schema rejection and replacement cleanup.

This is deliberately not yet marked as transactional paper-ledger integration. The store is compiled and validated, but the paper mutation path still needs to persist the account and risk snapshot as one recoverable transaction (or with a journal/commit marker) before entry admission can rely on the sidecar after every crash boundary. Until that wiring is finished, startup reconstruction from closed-equity history cannot recover a drawdown that breached and fully recovered before a crash without leaving a closed-equity trace.

## Alerts and next work

Existing signal/TP/SL/feed alarms remain unchanged. The next checkpoint is to wire the revision-bound store into paper mutations/recovery with an explicit crash protocol, then emit a dedicated risk-halt alarm/UI state without blocking monitoring or exits for positions that were already open.
