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

## Session breaker persistence and crash protocol

`RiskStateSnapshot` is versioned and validates the session start, observed peak, latest equity, limits, timestamp, and latched halt reason. A latched breaker never clears merely because equity recovers.

`RiskStateStore` provides an atomic, fsync-backed sidecar bound to the paper account revision. It validates the nested risk snapshot and fails closed on corrupt JSON, unknown schema versions, unsafe/unlatched breached snapshots, or a paper/risk revision mismatch.

The write-ahead `RiskCommitIntent` protocol covers the dangerous crash window between the paper-ledger rename and risk-sidecar rename. A caller prepares an intent for exactly `revision N -> N+1` before mutating the paper account. Normal sidecar loads refuse to proceed while an unfinished intent exists. On restart, recovery is deterministic:

- account still at N: the account mutation never committed, so the intent is aborted and the prior risk state remains authoritative;
- account at N+1: the account commit won the race, so recovery completes the prepared risk snapshot at N+1 and removes the intent;
- account at any other revision: state is ambiguous/corrupt, recovery fails closed and preserves the intent for investigation.

The protocol is now also bound to the *durable source risk state*. Preparing an ordinary mutation requires an existing sidecar at exactly the supplied source revision; missing sidecars and stale/future source revisions fail closed instead of silently creating a transaction from an unverified state. Ordinary mutations must preserve configured limits and session-start equity, cannot move the observation timestamp backwards, cannot lower the remembered equity peak, and cannot clear or replace an already latched halt. This closes a loophole where a stale or buggy caller could otherwise prepare a syntactically valid N -> N+1 intent that discarded safety memory.

Initialization is explicit and single-use. Session reset is also explicit: `begin_session_reset` is the only prepared transition allowed to clear the prior session's peak/latch, and it remains revision-bound and crash-recoverable. Recovery of an unapplied intent revalidates the durable source sidecar before deleting the intent, so corruption is not hidden by cleanup.

Both the intent and risk snapshot use atomic temporary-file replacement, file fsync, and containing-directory fsync where supported. Tests cover exact durable-source binding, missing/stale source rejection, monotonic peak/timestamp/latch rules, explicit reset semantics, prepared-intent admission blocking, unapplied-intent abort, post-account-commit completion, ambiguous-revision failure with evidence preservation, exact-target enforcement, corruption/schema rejection, revision mismatch, durable latch round-trip, and temp-file cleanup.

The protocol primitive is compiled and validated, but the paper mutation methods still need to call `begin_commit` before their durable ledger write and `finish_commit` after it, with startup calling `recover` before admission. Until that final wiring is complete, startup reconstruction from closed-equity history remains the active fallback and cannot recover an intratrade drawdown that breached and fully recovered before a crash without leaving a closed-equity trace.

## Alerts and next work

Existing signal/TP/SL/feed alarms remain unchanged. The next checkpoint is to wire this commit protocol through paper enter/observe/close/reset and startup recovery, then emit a dedicated risk-halt alarm/UI state without blocking monitoring or exits for positions that were already open.
