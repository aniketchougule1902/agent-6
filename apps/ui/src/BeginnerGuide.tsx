import type { Guidance } from "./tradeGuidance";
import type { TradeSignal } from "./generated/TradeSignal";
import type { EngineEvent } from "./generated/EngineEvent";
import { price } from "./MarketTools";

export function BeginnerGuide({ guidance, signal, event, acknowledged, onAcknowledge }: {
  guidance: Guidance; signal: TradeSignal | null; event: EngineEvent | null; acknowledged: boolean; onAcknowledge: () => void;
}) {
  return <section className={`beginner-guide guidance-${guidance.state}`} aria-label="What to do now">
    <div className="guide-heading"><span className="eyebrow">YOUR NEXT STEP</span><span className="practice-badge">PAPER PRACTICE · NO REAL ORDERS</span></div>
    <div role="status" aria-live="polite"><h1>{guidance.title}</h1><p>{guidance.detail}</p></div>
    {signal && <div className="guide-levels">
      <div><span>Direction · {signal.timeframe}m setup</span><strong>{signal.side === "long" ? "LONG · price rising" : "SHORT · price falling"}</strong></div>
      <div><span>Entry zone</span><strong>{price(signal.entry_low)} – {price(signal.entry_high)}</strong></div>
      <div><span>Stop loss · planned exit if wrong</span><strong>{price(signal.stop_loss)}</strong></div>
      <div><span>Targets · first / final</span><strong>{price(signal.tp1)} / {price(signal.tp2)}</strong></div>
    </div>}
    <p className="guide-trust">Accuracy is not established. Setup quality is not a win probability. Stops can slip and losses can exceed the planned amount.</p>
    {event && !acknowledged && <div className="priority-alert" role="alert"><div><strong>{event.signal?.symbol ?? "Market feed"} · {event.event_type.replaceAll("_", " ").toUpperCase()}</strong><p>{event.message}</p><small>{new Date(event.ts_ms).toLocaleTimeString()} · Acknowledging only dismisses this message.</small></div><button className="button" onClick={onAcknowledge}>Acknowledge</button></div>}
  </section>;
}
