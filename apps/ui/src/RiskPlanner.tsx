import { useState } from "react";
import type { TradeSignal } from "./generated/TradeSignal";
import type { Instrument } from "./MarketTools";
import { price } from "./MarketTools";
export function RiskPlanner({signal,instrument}:{signal:TradeSignal|null;instrument?:Instrument}) {
 const [equity,setEquity]=useState(1000);const [risk,setRisk]=useState(0.5);const [cost,setCost]=useState(12);
 const entry=signal?(signal.entry_low+signal.entry_high)/2:0;
 const distance=signal?Math.abs(entry-signal.stop_loss):0;
 const perUnit=distance+entry*cost/10000;
 const valid=equity>0&&risk>0&&risk<=5&&cost>=0&&Number.isFinite(perUnit)&&perUnit>0;
 const raw=valid?equity*risk/100/perUnit:0;
 const step=Number(instrument?.qty_step||0);
 const qty=step>0?Math.floor(raw/step)*step:raw;
 return <section className="panel risk-planner"><div className="eyebrow">POSITION PLANNER · PAPER</div><div className="planner-inputs">
 <label>Account (quote)<input type="number" min="1" value={equity} onChange={e=>setEquity(Number(e.target.value))}/></label>
 <label>Risk %<input type="number" min="0.01" max="5" step="0.1" value={risk} onChange={e=>setRisk(Number(e.target.value))}/></label>
 <label>Costs (bps)<input type="number" min="0" value={cost} onChange={e=>setCost(Number(e.target.value))}/></label></div>
 {signal&&valid?<><div className="planner-output"><span>Quantity <b>{price(qty)}</b></span><span>Notional <b>{price(qty*entry)}</b></span><span>Est. stop + costs <b>{price(qty*perUnit)}</b></span></div><p className="muted">{qty<Number(instrument?.min_qty??0)?'Below venue minimum quantity. ':''}Uses {instrument?'venue quantity increments':'unrounded quantity'}. Excludes funding, gap risk and liquidation. No order submitted.</p></>:<p className="muted">{signal?'Enter valid account, costs and risk (0–5%).':'Sizing appears when a setup is admitted.'}</p>}
 </section>;
}
