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
const money = (value: number) => (Math.abs(value) < 0.005 ? 0 : value).toLocaleString("en-US", {minimumFractionDigits: 2, maximumFractionDigits: 2});

function formatDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}
function pnlClass(v: number): string { return v > 0.005 ? "positive" : v < -0.005 ? "negative" : ""; }

export function PaperTrading({ snapshot, signal, entryAllowed, entryTitle, entryDetail }: {
  snapshot: EngineSnapshot; signal: TradeSignal | null; entryAllowed: boolean; entryTitle: string; entryDetail: string;
}) {
  const [view, setView] = useState<PaperView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastAction, setLastAction] = useState<string>("");
  const [notional, setNotional] = useState(() => {
    const stored = localStorage.getItem("a6_paper_notional");
    const parsed = stored ? Number(stored) : 100;
    return Number.isFinite(parsed) && parsed > 0 ? parsed : 100;
  });
  const [autoEnter, setAutoEnter] = useState(false);
  const [showClosed, setShowClosed] = useState(false);
  const [resetConfirm, setResetConfirm] = useState(false);
  const lastAutoSignal = useRef("");

  const refresh = useCallback(async () => {
    try {
      const r = await fetch(PAPER_API);
      if (!r.ok) throw new Error("Paper account unavailable");
      setView(await r.json()); setError(null);
    } catch { setError("Paper account unavailable; displayed values may be stale"); }
  }, []);
  useEffect(() => { void refresh(); const t = window.setInterval(refresh, 3000); return () => window.clearInterval(t); }, [refresh]);
  useEffect(() => { localStorage.setItem("a6_paper_notional", String(notional)); }, [notional]);

  const enter = useCallback(async (signalId: string, amt: number): Promise<boolean> => {
    setBusy(true); setError(null); setLastAction("Submitting paper entry…");
    try {
      const r = await fetch(`${PAPER_API}/enter`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ signal_id: signalId, notional: amt }) });
      const v = await r.json();
      if (!r.ok) throw new Error(v.error ?? "Enter failed");
      setView(v); setLastAction("Paper entry executed."); return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : "Enter failed"); setLastAction(""); return false;
    } finally { setBusy(false); }
  }, []);

  const close = useCallback(async (positionId: string) => {
    setBusy(true); setError(null); setLastAction("Closing paper position…");
    try {
      const r = await fetch(`${PAPER_API}/close`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ position_id: positionId }) });
      const v = await r.json();
      if (!r.ok) throw new Error(v.error ?? "Close failed");
      setView(v); setLastAction("Paper position closed.");
    } catch (e) { setError(e instanceof Error ? e.message : "Close failed"); setLastAction(""); }
    finally { setBusy(false); }
  }, []);

  const closeAll = useCallback(async (ids: string[]) => {
    if (!ids.length) return;
    setBusy(true); setError(null); setLastAction(`Closing ${ids.length} paper position${ids.length===1?"":"s"}…`);
    try {
      let latest: PaperView | null = null;
      for (const position_id of ids) {
        const r = await fetch(`${PAPER_API}/close`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ position_id }) });
        const v = await r.json();
        if (!r.ok) throw new Error(v.error ?? "Close all failed");
        latest = v;
      }
      if (latest) setView(latest);
      setLastAction("All open paper positions closed.");
    } catch (e) { setError(e instanceof Error ? e.message : "Close all failed"); setLastAction(""); }
    finally { setBusy(false); }
  }, []);

  const reset = useCallback(async () => {
    if (!view) return;
    setBusy(true); setError(null); setLastAction("Resetting paper account…");
    try {
      const r = await fetch(`${PAPER_API}/reset`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ confirmation: "RESET", revision: view.account.revision }) });
      const v = await r.json();
      if (!r.ok) throw new Error(v.error ?? "Reset failed");
      setView(v); setResetConfirm(false); setLastAction("Paper account reset.");
    } catch (e) { setError(e instanceof Error ? e.message : "Reset failed"); setLastAction(""); }
    finally { setBusy(false); }
  }, [view]);

  useEffect(() => {
    if (!entryAllowed || !autoEnter || busy || !view || !signal || signal.status !== "active" || snapshot.feed_stale || !snapshot.connected) return;
    if (signal.id === lastAutoSignal.current) return;
    if (view.account.positions.some(p => p.signal_id === signal.id)) { lastAutoSignal.current = signal.id; return; }
    const amt = notional > 0 ? notional : 100;
    void enter(signal.id, amt).then(ok => { if (ok) lastAutoSignal.current = signal.id; });
  }, [entryAllowed, autoEnter, signal, enter, notional, view, busy, snapshot.feed_stale, snapshot.connected]);

  const now = Date.now();
  const open = view?.account.positions.filter(p => p.remaining > 0) ?? [];
  const closed = view?.account.positions.filter(p => p.closed_ms != null).sort((a, b) => (b.closed_ms ?? 0) - (a.closed_ms ?? 0)) ?? [];
  const alreadyEntered = !!(signal && view?.account.positions.some(p => p.signal_id === signal.id));
  const canEnter = !!(entryAllowed && view && !error && !view.error && signal && signal.status === "active" && !alreadyEntered);
  const gateState = canEnter ? "ready" : snapshot.feed_stale || !snapshot.connected ? "halt" : "wait";
  const presets = [25, 50, 100, 250];

  return <section className="panel paper-panel execution-console">
    <div className="paper-header">
      <div><div className="eyebrow">PAPER EXECUTION CONSOLE</div><h3>Order actions & position control</h3></div>
      <div className="paper-actions">
        <button className="button compact" onClick={() => void refresh()} disabled={busy}>Refresh</button>
        <label className="auto-toggle" title="Automatically enter a paper position when the currently selected signal passes every entry gate">
          <input type="checkbox" checked={autoEnter} onChange={e => setAutoEnter(e.target.checked)} /> Auto-enter
        </label>
        {open.length>0 && <button className="button danger-action" onClick={() => void closeAll(open.map(p=>p.id))} disabled={busy}>Close all</button>}
        {!resetConfirm
          ? <button className="button paper-reset-btn" onClick={() => setResetConfirm(true)} disabled={busy}>Reset</button>
          : <span className="reset-confirm"><button className="button paper-reset-confirm" onClick={reset} disabled={busy}>Confirm reset</button><button className="button" onClick={() => setResetConfirm(false)}>Cancel</button></span>}
      </div>
    </div>

    <div className="paper-balance-bar">
      <div className="paper-stat"><span>Balance</span><strong>$${money(snapshot.paper_balance)}</strong></div>
      <div className="paper-stat"><span>Equity</span><strong className={pnlClass(snapshot.paper_equity - 1000)}>$${money(snapshot.paper_equity)}</strong></div>
      <div className="paper-stat"><span>Unrealized</span><strong className={pnlClass(snapshot.paper_unrealized)}>{snapshot.paper_unrealized >= 0 ? "+" : ""}{money(snapshot.paper_unrealized)}</strong></div>
      <div className="paper-stat"><span>Realized P&L</span><strong className={pnlClass(snapshot.paper_realized)}>{snapshot.paper_realized >= 0 ? "+" : ""}{money(snapshot.paper_realized)}</strong></div>
    </div>

    <div className="paper-analytics">
      <span>Trades <b>{snapshot.paper_closed_count}</b></span><span>Win rate <b>{snapshot.paper_win_rate != null ? `${(snapshot.paper_win_rate * 100).toFixed(1)}%` : "—"}</b></span>
      <span>Profit factor <b>{snapshot.paper_profit_factor != null ? snapshot.paper_profit_factor.toFixed(2) : "—"}</b></span><span>Fees <b>{money(snapshot.paper_fees)}</b></span>
      <span>Available <b>$${money(view?.available??0)}</b></span><span>Reserved <b>$${money(view?.reserved??0)}</b></span>
    </div>

    {(error||view?.error)&&<div className="paper-error">{error||view?.error}</div>}
    {lastAction && <div className="execution-toast" aria-live="polite">{lastAction}</div>}

    <div className={`execution-ticket gate-${gateState}`}>
      <div className="execution-ticket-head">
        <div><span className="eyebrow">SELECTED SIGNAL</span><strong className={signal?.side==="long"?"positive":signal?.side==="short"?"negative":""}>{signal ? `${signal.side.toUpperCase()} · ${signal.timeframe}m` : "NO ACTIVE SIGNAL"}</strong></div>
        <span className="gate-pill">{canEnter ? "ENTRY READY" : alreadyEntered ? "ALREADY EXECUTED" : "ENTRY BLOCKED"}</span>
      </div>
      {signal ? <div className="execution-levels">
        <span>Entry <b>{price((signal.entry_low+signal.entry_high)/2)}</b></span><span>Stop <b>{price(signal.stop_loss)}</b></span>
        <span>TP1 <b>{price(signal.tp1)}</b></span><span>TP2 <b>{price(signal.tp2)}</b></span><span>R:R <b>1:{signal.risk_reward_tp2.toFixed(2)}</b></span>
      </div> : <p className="muted">The execution ticket stays visible. It activates automatically when an admitted paper setup is selected.</p>}
      <div className="execution-gate-copy"><strong>{alreadyEntered ? "This signal already has a paper position." : entryTitle}</strong><span>{alreadyEntered ? "Use the open-position controls below to manage or close it." : entryDetail}</span></div>

      <div className="execution-controls">
        <div className="notional-block">
          <label>Notional $
            <input type="number" min={1} step={1} value={notional} onChange={e => { const v=Number(e.target.value); setNotional(Number.isFinite(v)&&v>0?v:1); }} />
          </label>
          <div className="notional-presets">{presets.map(v=><button key={v} className={notional===v?"active":""} onClick={()=>setNotional(v)} disabled={busy}>$${v}</button>)}</div>
        </div>
        <button className={`execute-button ${signal?.side==="short"?"short":"long"}`} disabled={!canEnter||busy} onClick={() => signal && void enter(signal.id, notional)}>
          <span>{busy ? "WORKING…" : signal ? `EXECUTE PAPER ${signal.side.toUpperCase()}` : "WAITING FOR SIGNAL"}</span>
          <small>{signal ? `$${money(notional)} notional · market simulation` : "No order will be sent"}</small>
        </button>
      </div>
      <p className="execution-footnote">Paper only. The engine submits no real exchange order. Auto-enter works only while this local UI is running.</p>
    </div>

    {open.length > 0 ? <>
      <div className="paper-section-title section-heading">OPEN POSITIONS <span>{open.length}</span></div>
      <div className="paper-positions">
        {open.map(p => {
          const dir = p.side === "long" ? 1 : -1;
          const unrealized = (p.mark - p.entry) * dir * p.remaining;
          const unrealizedPct = p.entry > 0 && p.remaining>0 ? (unrealized / (p.entry * p.remaining)) * 100 : 0;
          const stopDistance=Math.abs(p.entry-p.stop);
          const progress=stopDistance>0 ? ((p.mark-p.entry)*dir/stopDistance) : 0;
          return <div className="paper-position" key={p.id}>
            <div className="pp-header"><span className={p.side === "long" ? "positive" : "negative"}>{p.side.toUpperCase()}</span><span>{p.symbol} · {p.timeframe}m</span><span className="pp-time">{formatDuration(now - p.opened_ms)}</span></div>
            <div className="position-progress"><i style={{width:`${Math.max(0,Math.min(100,50+progress*20))}%`}} /></div>
            <div className="pp-levels"><span>Entry <b>{price(p.entry)}</b></span><span>Mark <b>{price(p.mark)}</b></span><span>SL <b>{price(p.stop)}</b></span><span>TP1 <b>{price(p.tp1)}</b> {p.tp1_done && <em>✓</em>}</span><span>TP2 <b>{price(p.tp2)}</b></span><span>Qty <b>{p.remaining.toFixed(4)}</b></span></div>
            <div className="pp-footer"><span className={pnlClass(unrealized)}>{unrealized >= 0 ? "+" : ""}{price(unrealized)} ({unrealizedPct >= 0 ? "+" : ""}{unrealizedPct.toFixed(2)}%)</span><button className="button pp-close" onClick={() => void close(p.id)} disabled={busy}>Close position</button></div>
          </div>;
        })}
      </div>
    </> : <div className="empty-position-state"><strong>No open paper positions</strong><span>Execution controls remain armed for the next qualified signal.</span></div>}

    {closed.length > 0 && <details open={showClosed} onToggle={e => setShowClosed((e.target as HTMLDetailsElement).open)}>
      <summary className="paper-section-title clickable">CLOSED TRADES · {closed.length}</summary>
      <div className="paper-closed">{closed.map(p => <div className="paper-closed-row" key={p.id}><span className={p.side === "long" ? "positive" : "negative"}>{p.side.toUpperCase()}</span><span>{p.symbol} · {p.timeframe}m</span><span>{p.exit_reason?.replaceAll("_", " ").toUpperCase()}</span><span className={pnlClass(p.realized_pnl)}>{p.realized_pnl >= 0 ? "+" : ""}{price(p.realized_pnl)}</span><span className="muted">{p.closed_ms ? new Date(p.closed_ms).toLocaleTimeString() : ""}</span></div>)}</div>
    </details>}

    {view&&view.account.fills.length>0&&<details><summary className="paper-section-title clickable">EXECUTION LEDGER · {view.account.fills.length} FILLS</summary><div className="paper-ledger">{view.account.fills.slice(-40).reverse().map((f,i)=><div key={`${f.ts_ms}-${i}`}><time>{new Date(f.ts_ms).toLocaleTimeString()}</time><strong>{f.kind.toUpperCase()}</strong><span>{price(f.quantity)} @ {price(f.price)}</span><span>Fee {f.fee.toFixed(4)}</span><span className={pnlClass(f.pnl)}>Net {f.pnl.toFixed(4)}</span></div>)}</div></details>}
    {!view && <p className="muted">Loading paper account…</p>}
  </section>;
}
