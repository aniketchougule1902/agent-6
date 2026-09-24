import type { EngineSnapshot } from "./generated/EngineSnapshot";
import type { TradeSignal } from "./generated/TradeSignal";

export type Guidance = { state: "halt" | "wait" | "track" | "ready"; title: string; detail: string; canEnter: boolean };
// A connected socket alone does not prove that snapshots are still arriving.
export function tradeGuidance(snapshot: EngineSnapshot, signal: TradeSignal | null, connected: boolean, now: number, receivedAt: number): Guidance {
  const result = (state: Guidance["state"], title: string, detail: string): Guidance => ({ state, title, detail, canEnter: state === "ready" });
  if (!connected || !snapshot.connected || snapshot.feed_stale || now - receivedAt > 5000 || now - snapshot.updated_at_ms > 5000 || snapshot.updated_at_ms > now + 5000)
    return result("halt", "Do not enter — live data unavailable", "Prices and setup levels may be old. Wait for fresh data; check any existing position separately.");
  if (!snapshot.features || !Number.isFinite(snapshot.last_price) || snapshot.last_price! <= 0)
    return result("wait", "Wait — market is warming up", "The engine needs fresh prices and enough closed candles before it can evaluate a setup.");
  if (!signal || signal.symbol !== snapshot.symbol)
    return result("wait", "Wait — no qualified setup", "No action needed. The engine is checking trend, order flow, costs and market conditions.");
  if (signal.status !== "active")
    return result("track", signal.status === "tp1_hit" ? "First target reached — monitor only" : "Setup ended — wait for a new one", "These levels describe an existing or completed setup. Do not use them for a new entry.");
  const age = now - signal.created_at_ms;
  if (age < 0 || age > 60_000)
    return result("track", "Entry window closed — do not chase", "New paper entries are limited to the first 60 seconds. Existing setup monitoring continues.");
  const p = snapshot.last_price!;
  if (![signal.entry_low, signal.entry_high, signal.stop_loss, signal.tp1, signal.tp2].every(v => Number.isFinite(v) && v > 0) || signal.entry_low > signal.entry_high)
    return result("halt", "Do not enter — invalid setup levels", "Wait for a valid new setup.");
  const entryMid = (signal.entry_low + signal.entry_high) / 2;
  const explicitHalfRange = Math.max(0, (signal.entry_high - signal.entry_low) / 2);
  const plannedRisk = Math.abs(entryMid - signal.stop_loss);
  const executionTolerance = Math.max(explicitHalfRange, plannedRisk * 0.15, entryMid * 0.0005);
  if (Math.abs(p - entryMid) > executionTolerance)
    return result("wait", "Wait — price moved beyond the execution window", `Paper execution allows a small live tolerance around the recorded entry, but the market has moved ${Math.abs(p-entryMid).toFixed(4)} away. Do not chase.`);
  const d = signal.side === "long" ? 1 : -1;
  if (d * (p - signal.stop_loss) <= 0 || d * (signal.tp1 - p) <= 0 || d * (signal.tp2 - signal.tp1) <= 0)
    return result("halt", "Do not enter — levels already crossed", "The stop or target no longer supports a new entry.");
  return result("ready", `${signal.side === "long" ? "Long" : "Short"} setup — paper practice only`, "Fresh setup inside its entry zone. Review the stop and position size before entering. A signal can lose.");
}
