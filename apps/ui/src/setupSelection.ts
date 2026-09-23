import type { TradeSignal } from './generated/TradeSignal';
export const isOpenSetup=(s:TradeSignal)=>s.status==='active'||s.status==='tp1_hit';
export function selectSetup(signals:TradeSignal[],chartTimeframe:string,focus:string):TradeSignal|null {
 const chosen=focus==='auto'?null:signals.find(s=>s.timeframe===focus);
 if(chosen)return chosen;
 return signals.find(s=>s.timeframe===chartTimeframe&&isOpenSetup(s))
   ?? signals.filter(isOpenSetup).sort((a,b)=>b.created_at_ms-a.created_at_ms||Number(a.timeframe)-Number(b.timeframe))[0]
   ?? signals.find(s=>s.timeframe===chartTimeframe)
   ?? signals.slice().sort((a,b)=>b.last_event_ms-a.last_event_ms)[0] ?? null;
}
