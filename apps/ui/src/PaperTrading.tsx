import { useCallback, useEffect, useRef, useState } from "react";
import type { EngineSnapshot } from "./generated/EngineSnapshot";
import type { TradeSignal } from "./generated/TradeSignal";
import { price } from "./MarketTools";

interface PaperPosition {
  id: string; signal_id: string; symbol: string; timeframe: string; side: string;
  quantity: number; remaining: number; entry: number; stop: number; tp1: number; tp2: number;
  opened_ms: number; expires_ms: number; mark: number; mark_ms: number; tp1_done: boolean;
  realized_pnl: number; fees: number; closed_ms: number | null; exit_reason: string | null;
}
interface PaperFill {
  position_id: string; ts_ms: number; kind: string; price: number; quantity: number; fee: number; pnl: number;
}
interface PaperAccount {
  initial_balance: number; balance: number; positions: PaperPosition[]; fills: PaperFill[];
  reset_ms: number; revision: number;
}
interface PaperView {
  account: PaperAccount; equity: number; available: number; reserved: number;
  unrealized: number; realized: number; fees: number; closed_trades: number;
  wins: number; win_rate: number | null; profit_factor: number | null;
  max_closed_drawdown: number; monitoring: string; error: string | null;
}

const PAPER_API = "/api/paper";

function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

function pnlClass(v: number): string {
  return v > 0.005 ? "positive" : v < -0.005 ? "negative" : "";
}

export function PaperTrading({ snapshot, signal }: { snapshot: EngineSnapshot; signal: TradeSignal | null }) {
  const [view, setView] = useState<PaperView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notional, setNotional] = useState(() => {
    const stored = localStorage.getItem("a6_paper_notional");
    return stored ? Number(stored) : 100;
  });
  const [autoEnter, setAutoEnter] = useState(false);
  const [showClosed, setShowClosed] = useState(false);
  const [resetConfirm, setResetConfirm] = useState(false);
  const lastAutoSignal = useRef("");

  const refresh = useCallback(async () => {
    try {
      const r = await fetch(PAPER_API);
      if (r.ok) { setView(await r.json()); }
    } catch { setError("Paper account unavailable; displayed values may be stale"); }
  }, []);

  useEffect(() => { void refresh(); const t = setInterval(refresh, 3000); return () => clearInterval(t); }, [refresh]);

  useEffect(() => { localStorage.setItem("a6_paper_notional", String(notional)); }, [notional]);


  const enter = useCallback(async (signalId: string, amt: number) => {
    setBusy(true); setError(null);
    try {
      const r = await fetch(`${PAPER_API}/enter`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ signal_id: signalId, notional: amt }) });
      const v = await r.json();
      if (!r.ok) throw new Error(v.error ?? "Enter failed");
      setView(v); setError(null);
    } catch (e) { setError(e instanceof Error ? e.message : "Enter failed"); }
    finally { setBusy(false); }
  }, []);

  const close = useCallback(async (positionId: string) => {
    setBusy(true); setError(null);
    try {
      const r = await fetch(`${PAPER_API}/close`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ position_id: positionId }) });
      const v = await r.json();
      if (!r.ok) throw new Error(v.error ?? "Close failed");
      setView(v); setError(null);
    } catch (e) { setError(e instanceof Error ? e.message : "Close failed"); }
    finally { setBusy(false); }
  }, []);

  const reset = useCallback(async () => {
    if (!view) return;
    setBusy(true); setError(null);
    try {
      const r = await fetch(`${PAPER_API}/reset`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ confirmation: "RESET", revision: view.account.revision }) });
      const v = await r.json();
      if (!r.ok) throw new Error(v.error ?? "Reset failed");
      setView(v); setError(null); setResetConfirm(false);
    } catch (e) { setError(e instanceof Error ? e.message : "Reset failed"); }
    finally { setBusy(false); }
  }, [view]);

  // Auto-enter on new signal
  useEffect(() => {
    if (!autoEnter || busy || !view || !signal || signal.status !== "active" || snapshot.feed_stale || !snapshot.connected) return;
    if (signal.id === lastAutoSignal.current) return;
    // Check if already entered this signal
    if (view?.account.positions.some(p => p.signal_id === signal.id)) {
      lastAutoSignal.current = signal.id;
      return;
    }
    lastAutoSignal.current = signal.id;
    const amt = notional > 0 ? notional : 100;
    void enter(signal.id, amt);
  }, [autoEnter, signal, enter, notional, view, busy, snapshot.feed_stale, snapshot.connected]);

  const now = Date.now();
  const open = view?.account.positions.filter(p => p.remaining > 0) ?? [];
  const closed = view?.account.positions.filter(p => p.closed_ms != null).sort((a, b) => (b.closed_ms ?? 0) - (a.closed_ms ?? 0)) ?? [];
  const canEnter = signal && signal.status === "active" && !view?.account.positions.some(p => p.signal_id === signal.id);

  return <section className="panel paper-panel">
    <div className="paper-header">
      <div className="eyebrow">PAPER TRADING · $1,000 START</div>
      <div className="paper-actions">
        <label className="auto-toggle" title="Automatically enter paper positions when new signals fire">
          <input type="checkbox" checked={autoEnter} onChange={e => setAutoEnter(e.target.checked)} />
          Auto-enter
        </label>
        {!resetConfirm
          ? <button className="button paper-reset-btn" onClick={() => setResetConfirm(true)} disabled={busy}>Reset</button>
          : <span className="reset-confirm">
              <button className="button paper-reset-confirm" onClick={reset} disabled={busy}>Confirm reset</button>
              <button className="button" onClick={() => setResetConfirm(false)}>Cancel</button>
            </span>
        }
      </div>
    </div>

    {/* Balance bar */}
    <div className="paper-balance-bar">
      <div className="paper-stat">
        <span>Balance</span>
        <strong>${price(snapshot.paper_balance)}</strong>
      </div>
      <div className="paper-stat">
        <span>Equity</span>
        <strong className={pnlClass(snapshot.paper_equity - 1000)}>${price(snapshot.paper_equity)}</strong>
      </div>
      <div className="paper-stat">
        <span>Unrealized</span>
        <strong className={pnlClass(snapshot.paper_unrealized)}>{snapshot.paper_unrealized >= 0 ? "+" : ""}{price(snapshot.paper_unrealized)}</strong>
      </div>
      <div className="paper-stat">
        <span>Realized P&L</span>
        <strong className={pnlClass(snapshot.paper_realized)}>{snapshot.paper_realized >= 0 ? "+" : ""}{price(snapshot.paper_realized)}</strong>
      </div>
    </div>

    {/* Analytics */}
    <div className="paper-analytics">
      <span>Trades <b>{snapshot.paper_closed_count}</b></span>
      <span>Win rate <b className={pnlClass((snapshot.paper_win_rate ?? 0) - 0.5)}>{snapshot.paper_win_rate != null ? `${(snapshot.paper_win_rate * 100).toFixed(1)}%` : "—"}</b></span>
      <span>Profit factor <b>{snapshot.paper_profit_factor != null ? snapshot.paper_profit_factor.toFixed(2) : "—"}</b></span>
      <span>Closed DD <b className="negative">{snapshot.paper_max_drawdown > 0 ? `-${price(snapshot.paper_max_drawdown)}` : "—"}</b></span>
      <span>Fees <b>{price(snapshot.paper_fees)}</b></span>
      <span>Open <b>{snapshot.paper_open_count}</b></span>
    </div>

    {(error||view?.error)&&<div className="paper-error">{error||view?.error}</div>}
    <p className="muted">Monitoring: {view?.monitoring??"connecting"} | Available ${price(view?.available??0)} | Reserved ${price(view?.reserved??0)}. Auto-entry only runs while this browser tab is open.</p>

    {/* Enter signal button */}
    {canEnter && <div className="paper-enter-row">
      <button className="button paper-enter-btn" disabled={busy} onClick={() => void enter(signal.id, notional)}>
        Enter {signal.side.toUpperCase()} · ${notional}
      </button>
      <label className="notional-input">
        Notional $
        <input type="number" min={1} value={notional} onChange={e => setNotional(Math.max(1, Number(e.target.value)))} />
      </label>
    </div>}

    {/* Open positions */}
    {open.length > 0 && <>
      <div className="paper-section-title">Open positions</div>
      <div className="paper-positions">
        {open.map(p => {
          const dir = p.side === "long" ? 1 : -1;
          const unrealized = (p.mark - p.entry) * dir * p.remaining;
          const unrealizedPct = p.entry > 0 ? (unrealized / (p.entry * p.remaining)) * 100 : 0;
          return <div className="paper-position" key={p.id}>
            <div className="pp-header">
              <span className={p.side === "long" ? "positive" : "negative"}>{p.side.toUpperCase()}</span>
              <span>{p.symbol} · {p.timeframe}m</span>
              <span className="pp-time">{formatDuration(now - p.opened_ms)}</span>
            </div>
            <div className="pp-levels">
              <span>Entry <b>{price(p.entry)}</b></span>
              <span>Mark <b>{price(p.mark)}</b></span>
              <span>SL <b>{price(p.stop)}</b></span>
              <span>TP1 <b>{price(p.tp1)}</b> {p.tp1_done && <em>✓</em>}</span>
              <span>TP2 <b>{price(p.tp2)}</b></span>
              <span>Qty <b>{p.remaining.toFixed(4)}</b></span>
            </div>
            <div className="pp-footer">
              <span className={pnlClass(unrealized)}>
                {unrealized >= 0 ? "+" : ""}{price(unrealized)} ({unrealizedPct >= 0 ? "+" : ""}{unrealizedPct.toFixed(2)}%)
              </span>
              <button className="button pp-close" onClick={() => void close(p.id)} disabled={busy}>Close</button>
            </div>
          </div>;
        })}
      </div>
    </>}

    {/* Closed positions toggle */}
    {closed.length > 0 && <details open={showClosed} onToggle={e => setShowClosed((e.target as HTMLDetailsElement).open)}>
      <summary className="paper-section-title clickable">{closed.length} closed trade{closed.length !== 1 ? "s" : ""}</summary>
      <div className="paper-closed">
        {closed.map(p => <div className="paper-closed-row" key={p.id}>
          <span className={p.side === "long" ? "positive" : "negative"}>{p.side.toUpperCase()}</span>
          <span>{p.symbol} · {p.timeframe}m</span>
          <span>{p.exit_reason?.replaceAll("_", " ").toUpperCase()}</span>
          <span className={pnlClass(p.realized_pnl)}>{p.realized_pnl >= 0 ? "+" : ""}{price(p.realized_pnl)}</span>
          <span className="muted">{p.closed_ms ? new Date(p.closed_ms).toLocaleTimeString() : ""}</span>
        </div>)}
      </div>
    </details>}

    {view&&view.account.fills.length>0&&<details><summary className="paper-section-title">Execution ledger ({view.account.fills.length} fills)</summary><div className="paper-ledger">{view.account.fills.slice(-30).reverse().map((f,i)=><div key={`${f.ts_ms}-${i}`}><time>{new Date(f.ts_ms).toLocaleTimeString()}</time><strong>{f.kind.toUpperCase()}</strong><span>{price(f.quantity)} @ {price(f.price)}</span><span>Fee {f.fee.toFixed(4)}</span><span className={pnlClass(f.pnl)}>Net {f.pnl.toFixed(4)}</span></div>)}</div></details>}
    {!view && <p className="muted">Loading paper account…</p>}
  </section>;
}
