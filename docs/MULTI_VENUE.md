# Multi-venue context integrity

Agent-6 treats secondary venues as **context**, not as an execution source and not as a replacement for the deterministic Bybit L50 signal path.

## Binance USD-M depth checkpoint

`apps/engine/src/multi_venue.rs` contains the first strict Binance USD-M depth adapter primitive. It preserves both exchange event time (`E`) and transaction time (`T`), update range (`U`/`u`), previous final update (`pu`), and zero-quantity deletion levels.

A websocket delta is not allowed to create a book from nothing. `start_binance_after_snapshot` requires a REST depth snapshot `lastUpdateId` and proves that the first buffered websocket event bridges `lastUpdateId + 1`. Subsequent deltas require `pu` continuity, monotonic final update IDs, and non-decreasing event time. A stale/duplicate update, sequence gap, backwards clock, malformed/non-finite level, or partial-book start fails closed. After a continuity failure, callers must discard local secondary-venue state and reacquire a snapshot.

The adapter is deliberately not connected to the production signal path yet. Live/recorded acquisition, reconnect/outage isolation, explicit cross-venue clock normalization, and causal lead/lag/spread/flow-divergence features remain separate checkpoints. This prevents an incomplete secondary feed from silently changing Bybit decisions.

## Safety boundary

- No credentials or private exchange APIs are required.
- No real-money execution is added or enabled.
- Secondary-venue observations must be recorded with their original exchange timestamps before research use.
- Historical/replay features must apply point-in-time availability rules; future secondary-venue observations must never be backfilled into earlier decisions.
- Any future hot-path use requires deterministic replay parity, outage tests, latency measurement, and explicit fail-closed semantics first.
