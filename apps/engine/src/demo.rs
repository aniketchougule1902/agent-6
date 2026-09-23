//! A deterministic software demonstration. Never published to the live journal.
use crate::{signal,simulator::{try_simulate_fill,FillRequest,LiquidityRole,SimulationConfig},state::InternalState,types::*};
use serde::Serialize;
#[derive(Serialize)]
pub struct Demo {
 pub disclosure:String,pub snapshot:EngineSnapshot,pub events:Vec<EngineEvent>,pub entry_fill:crate::simulator::SimulatedFill,pub exit_fills:Vec<crate::simulator::SimulatedFill>,pub gross_pnl:f64,pub fees:f64,pub net_pnl:f64,pub quantity:f64,
}
pub fn run(win:bool)->anyhow::Result<Demo>{
 let now=crate::state::now_ms()/60_000*60_000;
 let created=now-240_000;
 let signal=TradeSignal{id:"scripted-demo-only".into(),symbol:"DEMOUSDT".into(),timeframe:"1".into(),side:Side::Long,status:SignalStatus::Active,created_at_ms:created,entry_low:100.0,entry_high:100.0,stop_loss:99.0,tp1:101.4,tp2:102.5,risk_reward_tp2:2.5,confidence:0.0,calibrated:false,invalidation:"SCRIPTED DEMO: synthetic prices, manually seeded setup, no live strategy admission or orders.".into(),reasons:vec!["Deterministic lifecycle and fill simulation".into()],last_event_ms:created,observed_exit_price:None};
 let entry_event=EngineEvent{ts_ms:created,event_type:"signal".into(),alert:Some(AlertKind::Signal),message:"SCRIPTED DEMO: long 10 units at reference 100; no live order".into(),signal:Some(signal.clone())};
 let mut inner=InternalState::new();inner.signals.insert("1".into(),signal);
 let mut events=vec![entry_event];
 let path=if win{[100.0,100.6,101.4,101.9,102.5]}else{[100.0,100.2,99.7,99.2,98.9]};
 let mut candles=Vec::new();
 for i in 0..100 {
  let close=if i<95 {98.0+i as f64*0.02+(i as f64*0.4).sin()*0.12}else{path[i-95]};
  let open=candles.last().map(|c:&Candle|c.close).unwrap_or(close-0.05);
  let start=created-(95*60_000)+i as u64*60_000;
  candles.push(Candle{start_ms:start,end_ms:start+59_999,interval:"1".into(),open,high:open.max(close)+0.02,low:open.min(close)-0.02,close,volume:1000.0,turnover:1000.0*close,confirmed:true});
  if i>=95 {events.extend(signal::observe_price(&mut inner,close,start));}
 }
 let fill=|price:f64,side:i8,quantity:f64,stop:bool|try_simulate_fill(SimulationConfig::default(),FillRequest{side_sign:side,quantity,reference_price:price,best_bid:price-0.005,best_ask:price+0.005,visible_touch_quantity:10000.0,role:LiquidityRole::Taker,is_stop:stop,gap_price:if stop{Some(price)}else{None}});
 let entry_fill=fill(100.0,1,10.0,false)?;
 let mut remaining=entry_fill.filled_quantity;let mut exit_fills=Vec::new();
 for event in &events {
  let qty=match event.event_type.as_str(){"tp1"=>remaining/2.0,"tp2"|"stop_loss"=>remaining,_=>continue};
  let price=event.signal.as_ref().and_then(|s|s.observed_exit_price).unwrap();
  let exit=fill(price,-1,qty,event.event_type=="stop_loss")?;remaining-=exit.filled_quantity;exit_fills.push(exit);
 }
 let gross_pnl=exit_fills.iter().map(|f|(f.fill_price-entry_fill.fill_price)*f.filled_quantity).sum::<f64>();
 let fees=entry_fill.fee_quote+exit_fills.iter().map(|f|f.fee_quote).sum::<f64>();
 let final_signal=inner.signals.remove("1").unwrap();
 let flags=events.iter().filter_map(crate::flags::from_event).collect();
 let snapshot=EngineSnapshot{symbol:"DEMOUSDT".into(),timeframe:"1".into(),market_generation:0,connected:false,feed_stale:false,feed_age_ms:0,last_price:Some(path[4]),mark_price:None,index_price:None,features:None,active_signal:Some(final_signal.clone()),timeframe_signals:vec![final_signal],analyses:vec![],chart_flags:flags,candles_1m:candles,candles_3m:vec![],candles_5m:vec![],candles_15m:vec![],updated_at_ms:now};
 Ok(Demo{disclosure:format!("SCRIPTED {} DEMO — synthetic market path chosen in advance. Tests software behavior, not predictive accuracy. No live orders or journal entries.",if win{"WIN"}else{"LOSS"}),snapshot,events,entry_fill,exit_fills,gross_pnl,fees,net_pnl:gross_pnl-fees,quantity:10.0})
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn scripted_paths_exercise_production_lifecycle_and_fills(){
  let win=run(true).unwrap();assert!(win.net_pnl>0.0);assert_eq!(win.events.len(),3);assert_eq!(win.exit_fills.len(),2);assert!(win.fees>0.0);assert_eq!(win.snapshot.active_signal.unwrap().status,SignalStatus::Tp2Hit);
  let loss=run(false).unwrap();assert!(loss.net_pnl<0.0);assert_eq!(loss.exit_fills.len(),1);assert_eq!(loss.snapshot.active_signal.unwrap().status,SignalStatus::StopLossHit);
 }
}
