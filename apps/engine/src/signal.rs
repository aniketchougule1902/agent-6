use crate::{
    config::Config,
    runtime,
    state::{now_ms, AppState, InternalState, PendingSignalConfirmation},
    types::{
        AlertKind, Candle, EngineEvent, FeatureSnapshot, MarketRegime, Side, SignalStatus,
        TradeSignal, TimeframeAnalysis,
    },
};
use std::{collections::VecDeque, time::Duration};
use tokio::time;

pub async fn run_loop(state: AppState) {
    let mut ticker = time::interval(Duration::from_millis(400));
    loop {
        ticker.tick().await;
        let now = now_ms();
        let market = runtime::current();
        let events = evaluate_once(&state, now, &market);
        for event in events {
            state.publish(event);
        }
    }
}

/// Execute one production signal-engine evaluation tick at an explicit clock
/// value. Live mode calls this every 400ms; deterministic replay calls the same
/// function with its replay clock, avoiding a second research-only decision path.
pub fn evaluate_once(
    state: &AppState,
    now: u64,
    market: &runtime::RuntimeMarketConfig,
) -> Vec<EngineEvent> {
    let mut inner = state.inner.write();
    evaluate_internal(&mut inner, &state.config, now, market)
}

/// Pure production decision core shared by the live loop and replay. It mutates
/// only typed in-memory state and returns lifecycle/signal events to its caller.
pub(crate) fn evaluate_internal(
    inner: &mut InternalState,
    config: &Config,
    now: u64,
    market: &runtime::RuntimeMarketConfig,
) -> Vec<EngineEvent> {
    let mut events = Vec::new();
        inner.prune_flows(now);
        let stale = !inner.connected
            || now.saturating_sub(inner.last_market_event_ms) > config.stale_feed_ms
            || inner.orderbook_event_ms == 0
            || now.saturating_sub(inner.orderbook_event_ms) > config.stale_feed_ms;
        if stale != inner.feed_stale {
            inner.feed_stale = stale;
            events.push(EngineEvent {
                ts_ms: now,
                event_type: if stale { "feed_stale" } else { "feed_recovered" }.into(),
                alert: Some(if stale { AlertKind::FeedStale } else { AlertKind::FeedRecovered }),
                message: if stale { "Feed integrity halt; new setups suspended." } else { "Fresh feed restored." }.into(),
                signal: None,
            });
        }
        for signal in inner.signals.values_mut() {
            if !terminal(signal) && now >= signal.created_at_ms + hold_ms(&signal.timeframe) {
                signal.status = SignalStatus::Expired;
                signal.last_event_ms = now;
                signal.observed_exit_price = None;
                events.push(EngineEvent { ts_ms:now,event_type:"expired".into(),alert:Some(AlertKind::Expired),message:format!("{}m setup expired; no execution implied",signal.timeframe),signal:Some(signal.clone()) });
            }
        }
        let mut analyses: Vec<_> = crate::indicators::TIMEFRAMES.iter().filter_map(|tf| {
            inner.bars.get(*tf).and_then(|bars| crate::indicators::analyze(bars,tf,now))
        }).collect();
        if let Some(features) = compute_features(&inner, now) {
            inner.features = Some(features.clone());
            let companions: Vec<_> = analyses.iter().map(|a| (
                a.timeframe.clone(), a.bias.clone(), a.quality, a.adx14, a.macd_histogram
            )).collect();
            for analysis in &mut analyses {
                let direction = if analysis.bias == "long" {1.0} else {-1.0};
                let (live_edge, live_votes) = directional_live_edge(&features, direction);
                let live_quality = (0.5 + 0.5*live_edge).clamp(0.0,1.0);
                analysis.quality = (0.85 * analysis.quality + 0.15 * live_quality).clamp(0.0,1.0);
                if stale { analysis.blockers.push("Live feed is stale".into()); }
                if features.spread_bps > config.max_spread_bps {analysis.blockers.push("Spread above limit".into());}
                if live_edge < config.min_live_edge {analysis.blockers.push(format!("Live microstructure edge is too weak ({live_edge:.2} < {:.2})",config.min_live_edge));}
                if live_votes < 3 {analysis.blockers.push("Live order-book/trade-flow consensus has fewer than 3 confirming components".into());}
                let one_minute_atr_bps = (features.atr_14/features.last_price*10_000.0).max(1.0);
                if direction*features.momentum_1m_bps < -0.35*one_minute_atr_bps {
                    analysis.blockers.push("1m momentum reversed against the setup before entry".into());
                }
                let higher = match analysis.timeframe.as_str() {"1"=>"3","3"=>"5","5"=>"15",_=>"5"};
                let companion_ok = companions.iter().any(|(tf,bias,quality,adx,macd)| {
                    tf==higher && bias==&analysis.bias && bias!="neutral"
                        && *quality>=0.50 && *adx>=18.0 && direction * *macd>0.0
                });
                if !companion_ok {analysis.blockers.push("Companion timeframe lacks trend/momentum confirmation".into());}
                if (features.last_price-analysis.close).abs()>analysis.atr14*0.60 {analysis.blockers.push("Live price moved too far from the closed candle".into());}
                if direction*(features.last_price-analysis.close) < -analysis.atr14*0.25 {
                    analysis.blockers.push("Price moved adversely after the signal candle closed".into());
                }
                if analysis.setup=="channel_breakout" {
                    let breakout_level=if direction>0.0 {analysis.resistance} else {analysis.support};
                    let hold=direction*(features.last_price-breakout_level)/analysis.atr14;
                    if hold<0.04 {analysis.blockers.push("Breakout level was not held after the close".into());}
                    if hold>1.00 {analysis.blockers.push("Breakout entry is overextended from the channel".into());}
                } else if analysis.setup=="trend_pullback" && direction*(features.last_price-analysis.ema21) < -0.10*analysis.atr14 {
                    analysis.blockers.push("Pullback lost EMA21 before entry".into());
                }
                if analysis.quality < config.min_signal_score {analysis.blockers.push("Quality below configured threshold".into());}
                if analysis.atr14/features.last_price*10_000.0>150.0 {analysis.blockers.push("Abnormal volatility".into());}
                let repeated=inner.admitted_candles.get(&analysis.timeframe)==Some(&analysis.candle_ms);
                if analysis.blockers.is_empty() {
                    if let Some(ref model) = inner.model {
                        let pred = model.predict(&features, analysis);
                        if pred.abstain {
                            analysis.blockers.push(format!(
                                "ML model abstention: TP2 outcome probability {:.1}% below threshold {:.1}%",
                                pred.calibrated_probability * 100.0,
                                pred.threshold * 100.0
                            ));
                        }
                    }
                }
                if analysis.blockers.is_empty() {
                    if !confirmation_ready(inner, analysis, now, config.signal_confirm_ms) {
                        let elapsed=inner.pending_signal_confirmations.get(&analysis.timeframe)
                            .map(|p| now.saturating_sub(p.first_pass_ms)).unwrap_or(0);
                        analysis.blockers.push(format!(
                            "Arming signal: live agreement held {}ms / {}ms",
                            elapsed.min(config.signal_confirm_ms), config.signal_confirm_ms
                        ));
                    }
                } else {
                    inner.pending_signal_confirmations.remove(&analysis.timeframe);
                }
                if analysis.blockers.is_empty() {
                    if let Some(signal)=build_signal(config,&features,analysis,&market.symbol,now,inner.model.as_ref()) {
                        if !repeated {
                            let (admit, transition) = prepare_signal_transition(
                                &mut *inner,
                                &signal,
                                features.last_price,
                                now,
                                config.signal_cooldown_secs.saturating_mul(1000),
                            );
                            if let Some(event) = transition {
                                events.push(event);
                            }
                            if admit {
                                inner.admitted_candles.insert(analysis.timeframe.clone(),analysis.candle_ms);
                                inner.signals.insert(analysis.timeframe.clone(),signal.clone());
                                events.push(EngineEvent {ts_ms:now,event_type:"signal".into(),alert:Some(AlertKind::Signal),message:format!("{} {}m {:?} paper setup; {} {:.0}%",signal.symbol,signal.timeframe,signal.side,if signal.calibrated {"calibrated prob"} else {"quality"},signal.confidence*100.0),signal:Some(signal)});
                            }
                        }
                    } else { analysis.blockers.push("Estimated costs or stop geometry fail risk checks".into()); }
                }
            }
        }
        inner.analyses=analyses;
        inner.active_signal=inner.signals.get(&market.timeframe).cloned();
        inner.updated_at_ms=now;

    events
}

fn compute_features(s: &InternalState, now: u64) -> Option<FeatureSnapshot> {
    let price = s.last_price?;
    let bars_1m = s.bars.get("1")?;
    let bars_5m = s.bars.get("5")?;
    let bars_15m = s.bars.get("15")?;
    if bars_1m.len() < 30 || bars_5m.len() < 22 || bars_15m.len() < 22 { return None; }
    let atr = atr(bars_1m, 14)?;
    let vwap = vwap(bars_1m, 20)?;
    let momentum = return_bps(bars_1m, 5).unwrap_or_default();
    let trend_5m = ema_spread_bps(bars_5m, 8, 21).unwrap_or_default();
    let trend_15m = ema_spread_bps(bars_15m, 8, 21).unwrap_or_default();
    let spread_bps = match (s.bid, s.ask) { (Some(bid), Some(ask)) if bid > 0.0 && ask >= bid => ((ask - bid) / ((ask + bid) / 2.0)) * 10_000.0, _ => 0.0 };
    let flow = normalized_flow(&s.trade_flow.iter().map(|x| x.signed_qty).collect::<Vec<_>>());
    let trade_metrics = trade_flow_metrics(&s.trade_flow, now);
    let liq = normalized_flow(&s.liquidation_flow.iter().map(|x| x.signed_qty).collect::<Vec<_>>());
    let liquidation_burst = liquidation_burst_5s(&s.liquidation_flow, now);
    let oi_delta_pct = match (s.previous_open_interest, s.open_interest) { (Some(prev), Some(current)) if prev.abs() > f64::EPSILON => (current - prev) / prev * 100.0, _ => 0.0 };
    let oi_delta_1m_pct = oi_window_delta_pct(&s.oi_samples, now, 60_000);
    let oi_delta_5m_pct = oi_window_delta_pct(&s.oi_samples, now, 300_000);
    let atr_bps = atr / price * 10_000.0;
    let regime = if atr_bps > 35.0 { MarketRegime::HighVolatility } else if trend_15m > 4.0 && trend_5m > 2.0 { MarketRegime::TrendingUp } else if trend_15m < -4.0 && trend_5m < -2.0 { MarketRegime::TrendingDown } else { MarketRegime::Range };
    let directional_edge = signed_unit(trend_15m / 12.0) * 0.08 + signed_unit(trend_5m / 10.0) * 0.07 + signed_unit(momentum / 25.0) * 0.07 + signed_unit(((price - vwap) / price * 10_000.0) / 12.0) * 0.07 + s.book_imbalance.clamp(-1.0, 1.0) * 0.07 + s.book_imbalance_top5.clamp(-1.0, 1.0) * 0.07 + signed_unit(s.microprice_bps / 1.5) * 0.04 + signed_unit((s.bid_depth_slope - s.ask_depth_slope) / 0.8) * 0.03 + s.depth_pressure.clamp(-1.0, 1.0) * 0.05 + flow * 0.11 + trade_metrics.large_trade_imbalance * 0.05 + liq * 0.03 + liquidation_burst * 0.04 + signed_unit(oi_delta_1m_pct / 0.08) * signed_unit(momentum / 20.0).abs() * 0.03 + signed_unit(oi_delta_5m_pct / 0.15) * signed_unit(trend_5m / 10.0).abs() * 0.02;
    let crowding_penalty = s.funding_rate.map(|f| (f.abs() / 0.001).clamp(0.0, 1.0) * 0.03).unwrap_or_default();
    let long_score = (0.5 + directional_edge - crowding_penalty).clamp(0.0, 1.0);
    let short_score = (0.5 - directional_edge - crowding_penalty).clamp(0.0, 1.0);
    Some(FeatureSnapshot { ts_ms: now, feed_age_ms: now.saturating_sub(s.last_market_event_ms), orderbook_age_ms: now.saturating_sub(s.orderbook_event_ms), last_price: price, spread_bps, atr_14: atr, vwap_20: vwap, momentum_1m_bps: momentum, trend_5m_bps: trend_5m, trend_15m_bps: trend_15m, book_imbalance: s.book_imbalance, book_imbalance_top5: s.book_imbalance_top5, microprice_bps: s.microprice_bps, bid_depth_slope: s.bid_depth_slope, ask_depth_slope: s.ask_depth_slope, depth_pressure: s.depth_pressure, trade_flow_imbalance: flow, trade_velocity_5s: trade_metrics.velocity_per_sec, signed_notional_5s: trade_metrics.signed_notional, large_trade_imbalance: trade_metrics.large_trade_imbalance, liquidation_pressure: liq, liquidation_burst_5s: liquidation_burst, open_interest: s.open_interest, open_interest_delta_pct: oi_delta_pct, open_interest_delta_1m_pct: oi_delta_1m_pct, open_interest_delta_5m_pct: oi_delta_5m_pct, funding_rate: s.funding_rate, regime, long_score, short_score })
}

fn directional_live_edge(f:&FeatureSnapshot,direction:f64)->(f64,u8){
    let oi_with_move=(f.open_interest_delta_1m_pct/0.08).clamp(-1.0,1.0)
        * (f.momentum_1m_bps/10.0).clamp(-1.0,1.0);
    let components=[
        (f.trade_flow_imbalance,0.24),(f.book_imbalance_top5,0.20),
        (f.large_trade_imbalance,0.16),(f.depth_pressure,0.14),
        (f.book_imbalance,0.10),((f.microprice_bps/1.5).clamp(-1.0,1.0),0.08),
        (f.liquidation_burst_5s,0.05),(oi_with_move,0.03),
    ];
    let raw:f64=components.iter().map(|(value,weight)| (*value).clamp(-1.0,1.0) * *weight).sum();
    let votes=components.iter().filter(|(value,_)| direction * (*value)>0.03).count() as u8;
    ((direction*raw).clamp(-1.0,1.0),votes)
}
fn confirmation_ready(inner:&mut InternalState,a:&TimeframeAnalysis,now:u64,required_ms:u64)->bool{
    if required_ms==0{return true;}
    let side=a.bias.clone();let setup=a.setup.clone();
    let pending=inner.pending_signal_confirmations.entry(a.timeframe.clone()).or_insert_with(||PendingSignalConfirmation{candle_ms:a.candle_ms,side:side.clone(),setup:setup.clone(),first_pass_ms:now,pass_ticks:0});
    if pending.candle_ms!=a.candle_ms||pending.side!=side||pending.setup!=setup{
        *pending=PendingSignalConfirmation{candle_ms:a.candle_ms,side,setup,first_pass_ms:now,pass_ticks:0};
    }
    pending.pass_ticks=pending.pass_ticks.saturating_add(1);
    now.saturating_sub(pending.first_pass_ms)>=required_ms&&pending.pass_ticks>=3
}

fn build_signal(config:&Config, f:&FeatureSnapshot, a:&TimeframeAnalysis, symbol:&str, now:u64, model:Option<&crate::model::ModelEvaluator>) -> Option<TradeSignal> {
    let price=f.last_price; let side=if a.bias=="long" {Side::Long} else {Side::Short}; let direction=if a.bias=="long" {1.0} else {-1.0};
    let anchor=if a.setup=="channel_breakout" {if direction>0.0 {a.resistance} else {a.support}} else {a.ema21};
    let risk=(direction*(price-anchor)+a.atr14*0.35).max(a.atr14*1.2).max(price*0.0008); let cost=price*(config.round_trip_cost_bps+f.spread_bps)/10_000.0; let rr=config.min_rr.max(2.5);
    if !cost.is_finite() || cost<0.0 || risk>a.atr14*3.0 || risk<cost*1.25 || (risk*rr-cost)/(risk+cost)<config.min_rr {return None;}
    let tick=crate::catalog::tick_size(symbol)?; let (stop_loss,tp1,tp2)=rounded_levels(price,direction,risk,rr,tick)?; let actual_risk=(price-stop_loss).abs(); let actual_reward=(tp2-price).abs();
    if (actual_reward-cost)/(actual_risk+cost)<config.min_rr {return None;} let rr=actual_reward/actual_risk; if stop_loss<=0.0 || tp1<=0.0 || tp2<=0.0 {return None;}
    let (confidence, calibrated, ml_reasons) = if let Some(m) = model { let pred=m.predict(f,a); (pred.calibrated_probability,true,vec![format!("ML calibrated probability: {:.1}% (Brier {:.3}, ECE {:.3})",pred.calibrated_probability*100.0,pred.brier,pred.ece)]) } else {(a.quality,false,vec![])};
    let (live_edge,live_votes)=directional_live_edge(f,direction);
    let mut reasons=vec!["strategy:a6-live-v3".into(),format!("raw-quality:{:.10}",a.quality),a.setup.replace('_'," "),format!("Closed {}m candle",a.timeframe),format!("ADX {:.1} / RSI {:.1}",a.adx14,a.rsi14),format!("Relative volume {:.2}x",a.relative_volume),format!("Live microstructure edge {:+.2} with {}/8 confirming components",live_edge,live_votes),format!("Live agreement held at least {}ms",config.signal_confirm_ms),format!("Estimated round-trip costs {:.1} bps",config.round_trip_cost_bps+f.spread_bps)]; reasons.extend(ml_reasons);
    Some(TradeSignal { id:stable_signal_id(symbol,&a.timeframe,a.candle_ms,&side),symbol:symbol.into(),timeframe:a.timeframe.clone(),side,status:SignalStatus::Active,created_at_ms:now,last_event_ms:now,observed_exit_price:None,entry_low:price,entry_high:price,stop_loss,tp1,tp2,risk_reward_tp2:rr,confidence,calibrated,invalidation:format!("Paper reference entry. Stop {:.10}; expires in {} minutes. Net estimated TP2 R:R {:.2}.",stop_loss,hold_ms(&a.timeframe)/60_000,(actual_reward-cost)/(actual_risk+cost)),reasons })
}

fn prepare_signal_transition(inner:&mut InternalState,candidate:&TradeSignal,observed_price:f64,now:u64,cooldown_ms:u64)->(bool,Option<EngineEvent>){
    let Some(existing)=inner.signals.get(&candidate.timeframe).cloned() else{return(true,None);};
    if terminal(&existing){if now.saturating_sub(existing.last_event_ms)>cooldown_ms{inner.archive_signal(existing);return(true,None);}return(false,None);}
    if existing.side==candidate.side{return(false,None);}
    let mut reversed=existing; reversed.status=SignalStatus::Reversed; reversed.last_event_ms=now; reversed.observed_exit_price=Some(observed_price); reversed.invalidation=format!("Reversed explicitly at {:.10}: a new fully-admitted {:?} {}m setup replaced the prior {:?} thesis. Paper reference only.",observed_price,candidate.side,candidate.timeframe,reversed.side); inner.archive_signal(reversed.clone());
    (true,Some(EngineEvent{ts_ms:now,event_type:"reversed".into(),alert:Some(AlertKind::Reversed),message:format!("{} {}m {:?} setup explicitly reversed by new {:?} setup at {:.4}; old signal {} remains in history",reversed.symbol,reversed.timeframe,reversed.side,candidate.side,observed_price,reversed.id),signal:Some(reversed)}))
}
fn stable_signal_id(symbol:&str,timeframe:&str,candle_ms:u64,side:&Side)->String{let side=if matches!(side,Side::Long){"long"}else{"short"};format!("{symbol}:{timeframe}:{candle_ms}:{side}")}
fn rounded_levels(price:f64,direction:f64,risk:f64,rr:f64,tick:f64)->Option<(f64,f64,f64)>{if ![price,risk,rr,tick].iter().all(|v|v.is_finite()&&*v>0.0)||![1.0,-1.0].contains(&direction){return None;}let round=|v:f64,up:bool|if up{(v/tick).ceil()*tick}else{(v/tick).floor()*tick};let stop=round(price-direction*risk,direction<0.0);let risk=(price-stop).abs();let first=round(price+direction*risk*1.4,direction>0.0);let last=round(price+direction*risk*rr,direction>0.0);if ![stop,first,last].iter().all(|v|v.is_finite()&&*v>0.0)||direction*(first-price)<=0.0||direction*(last-first)<=0.0{return None;}Some((stop,first,last))}
fn terminal(signal:&TradeSignal)->bool{matches!(signal.status,SignalStatus::Tp2Hit|SignalStatus::StopLossHit|SignalStatus::Expired|SignalStatus::Reversed|SignalStatus::Invalidated)}
fn hold_ms(timeframe:&str)->u64{timeframe.parse::<u64>().unwrap_or(1).saturating_mul(4).clamp(5,60)*60_000}

pub fn observe_price(inner:&mut InternalState,price:f64,ts_ms:u64)->Vec<EngineEvent>{let mut events=vec![];if !price.is_finite()||price<=0.0{return events;}for signal in inner.signals.values_mut(){if terminal(signal)||ts_ms<signal.created_at_ms||ts_ms<signal.last_event_ms{continue;}let direction=if matches!(signal.side,Side::Long){1.0}else{-1.0};let stopped=direction*(price-signal.stop_loss)<=0.0;let final_target=direction*(price-signal.tp2)>=0.0;let first_target=direction*(price-signal.tp1)>=0.0;if ts_ms>=signal.created_at_ms+hold_ms(&signal.timeframe){signal.status=SignalStatus::Expired;signal.last_event_ms=ts_ms;signal.observed_exit_price=Some(price);events.push(lifecycle_event(signal,ts_ms,"expired",AlertKind::Expired,price));}else if stopped{signal.status=SignalStatus::StopLossHit;signal.last_event_ms=ts_ms;signal.observed_exit_price=Some(price);events.push(lifecycle_event(signal,ts_ms,"stop_loss",AlertKind::StopLoss,price));}else{if first_target&&signal.status==SignalStatus::Active{signal.status=SignalStatus::Tp1Hit;signal.last_event_ms=ts_ms;signal.observed_exit_price=Some(price);events.push(lifecycle_event(signal,ts_ms,"tp1",AlertKind::Tp1,price));}if final_target{signal.status=SignalStatus::Tp2Hit;signal.last_event_ms=ts_ms;signal.observed_exit_price=Some(price);events.push(lifecycle_event(signal,ts_ms,"tp2",AlertKind::Tp2,price));}}}events}
fn lifecycle_event(signal:&TradeSignal,now:u64,kind:&str,alert:AlertKind,price:f64)->EngineEvent{EngineEvent{ts_ms:now,event_type:kind.into(),alert:Some(alert),message:format!("{} {}m {} observed at {:.4} (paper)",signal.symbol,signal.timeframe,kind,price),signal:Some(signal.clone())}}
fn atr(bars:&std::collections::VecDeque<Candle>,period:usize)->Option<f64>{if bars.len()<period+1{return None;}let slice:Vec<_>=bars.iter().rev().take(period+1).collect();let mut sum=0.0;for pair in slice.windows(2){let current=pair[0];let previous=pair[1];let tr=(current.high-current.low).max((current.high-previous.close).abs()).max((current.low-previous.close).abs());sum+=tr;}Some(sum/period as f64)}
fn vwap(bars:&std::collections::VecDeque<Candle>,period:usize)->Option<f64>{let mut pv=0.0;let mut volume=0.0;for bar in bars.iter().rev().take(period){pv+=bar.close*bar.volume;volume+=bar.volume;}(volume>0.0).then_some(pv/volume)}
fn return_bps(bars:&std::collections::VecDeque<Candle>,lookback:usize)->Option<f64>{let current=bars.back()?.close;let previous=bars.get(bars.len().checked_sub(lookback+1)?)?.close;Some((current/previous-1.0)*10_000.0)}
fn ema_spread_bps(bars:&std::collections::VecDeque<Candle>,fast:usize,slow:usize)->Option<f64>{let closes:Vec<f64>=bars.iter().map(|x|x.close).collect();let price=*closes.last()?;let fast_ema=ema(&closes,fast)?;let slow_ema=ema(&closes,slow)?;Some((fast_ema-slow_ema)/price*10_000.0)}
fn ema(values:&[f64],period:usize)->Option<f64>{if values.len()<period{return None;}let alpha=2.0/(period as f64+1.0);let mut result=values[0];for value in &values[1..]{result=alpha**value+(1.0-alpha)*result;}Some(result)}
#[derive(Debug,Clone,Copy,Default)]struct TradeFlowMetrics{velocity_per_sec:f64,signed_notional:f64,large_trade_imbalance:f64}
fn trade_flow_metrics(samples:&VecDeque<crate::state::FlowSample>,now:u64)->TradeFlowMetrics{let cutoff=now.saturating_sub(5_000);let recent:Vec<_>=samples.iter().filter(|sample|sample.ts_ms>=cutoff).collect();if recent.is_empty(){return TradeFlowMetrics::default();}let signed_notional:f64=recent.iter().map(|sample|sample.signed_notional).sum();let mut notionals:Vec<f64>=recent.iter().map(|sample|sample.signed_notional.abs()).filter(|value|*value>0.0).collect();notionals.sort_by(|a,b|a.total_cmp(b));let large_trade_imbalance=if notionals.len()>=5{let threshold_index=((notionals.len()-1)*9)/10;let threshold=notionals[threshold_index];let mut signed=0.0;let mut absolute=0.0;for sample in &recent{let notional=sample.signed_notional;if notional.abs()>=threshold{signed+=notional;absolute+=notional.abs();}}if absolute>f64::EPSILON{(signed/absolute).clamp(-1.0,1.0)}else{0.0}}else{0.0};TradeFlowMetrics{velocity_per_sec:recent.len() as f64/5.0,signed_notional,large_trade_imbalance}}
fn liquidation_burst_5s(samples:&VecDeque<crate::state::FlowSample>,now:u64)->f64{let recent_cutoff=now.saturating_sub(5_000);let baseline_cutoff=now.saturating_sub(60_000);let mut recent_signed=0.0;let mut recent_abs=0.0;let mut baseline_abs=0.0;for sample in samples.iter().filter(|sample|sample.ts_ms>=baseline_cutoff){let notional=sample.signed_notional;if sample.ts_ms>=recent_cutoff{recent_signed+=notional;recent_abs+=notional.abs();}else{baseline_abs+=notional.abs();}}if recent_abs<=f64::EPSILON{return 0.0;}let direction=(recent_signed/recent_abs).clamp(-1.0,1.0);let baseline_per_5s=baseline_abs/11.0;let intensity=if baseline_per_5s<=f64::EPSILON{1.0}else{((recent_abs/baseline_per_5s)-1.0).max(0.0).tanh()};direction*intensity}
fn oi_window_delta_pct(samples:&VecDeque<crate::state::OiSample>,now:u64,window_ms:u64)->f64{let Some(current)=samples.back()else{return 0.0;};let cutoff=now.saturating_sub(window_ms);let Some(anchor)=samples.iter().find(|sample|sample.ts_ms>=cutoff)else{return 0.0;};if anchor.value.abs()<=f64::EPSILON||current.ts_ms.saturating_sub(anchor.ts_ms)<window_ms/3{return 0.0;}(current.value-anchor.value)/anchor.value*100.0}
fn normalized_flow(samples:&[f64])->f64{let signed:f64=samples.iter().sum();let absolute:f64=samples.iter().map(|x|x.abs()).sum();if absolute<=f64::EPSILON{0.0}else{(signed/absolute).clamp(-1.0,1.0)}}
fn signed_unit(value:f64)->f64{value.tanh()}

#[cfg(test)]mod tests{use super::*;fn test_signal(id:&str,side:Side,status:SignalStatus,last_event_ms:u64)->TradeSignal{TradeSignal{id:id.into(),symbol:"BTCUSDT".into(),timeframe:"1".into(),side,status,created_at_ms:1_000,entry_low:100.0,entry_high:100.0,stop_loss:99.0,tp1:101.0,tp2:102.5,risk_reward_tp2:2.5,confidence:0.7,calibrated:false,invalidation:"original".into(),reasons:vec![],last_event_ms,observed_exit_price:None}}
fn test_analysis()->TimeframeAnalysis{TimeframeAnalysis{timeframe:"15".into(),candle_ms:60_000,close:100.0,ema9:101.0,ema21:100.0,ema50:99.0,rsi14:60.0,adx14:30.0,macd_histogram:1.0,atr14:2.0,vwap20:99.5,bb_upper:104.0,bb_lower:96.0,relative_volume:1.8,support:95.0,resistance:99.0,bias:"long".into(),setup:"channel_breakout".into(),quality:0.8,blockers:vec![]}}
#[test]fn confirmation_requires_continuous_same_thesis_window(){let mut inner=InternalState::new();let mut a=test_analysis();assert!(!confirmation_ready(&mut inner,&a,1_000,6_000));assert!(!confirmation_ready(&mut inner,&a,3_000,6_000));assert!(confirmation_ready(&mut inner,&a,7_000,6_000));a.candle_ms+=60_000;assert!(!confirmation_ready(&mut inner,&a,8_000,6_000));}
#[test]fn opposite_admitted_candidate_explicitly_reverses_and_archives_old_signal(){let mut inner=InternalState::new();inner.signals.insert("1".into(),test_signal("old",Side::Long,SignalStatus::Active,1_000));let candidate=test_signal("new",Side::Short,SignalStatus::Active,2_000);let(admit,event)=prepare_signal_transition(&mut inner,&candidate,99.5,2_000,30_000);assert!(admit);let event=event.expect("reversal event");assert_eq!(event.event_type,"reversed");assert_eq!(event.signal.as_ref().unwrap().status,SignalStatus::Reversed);assert_eq!(inner.signal_history.len(),1);assert_eq!(inner.signal_history.back().unwrap().id,"old");assert_eq!(inner.signal_history.back().unwrap().observed_exit_price,Some(99.5));}
#[test]fn same_side_candidate_does_not_replace_active_signal(){let mut inner=InternalState::new();inner.signals.insert("1".into(),test_signal("old",Side::Long,SignalStatus::Active,1_000));let candidate=test_signal("new",Side::Long,SignalStatus::Active,2_000);let(admit,event)=prepare_signal_transition(&mut inner,&candidate,100.5,2_000,30_000);assert!(!admit);assert!(event.is_none());assert!(inner.signal_history.is_empty());assert_eq!(inner.signals["1"].id,"old");}
#[test]fn terminal_signal_is_archived_only_after_cooldown_before_replacement(){let mut inner=InternalState::new();inner.signals.insert("1".into(),test_signal("old",Side::Long,SignalStatus::StopLossHit,10_000));let candidate=test_signal("new",Side::Short,SignalStatus::Active,20_000);let(early,_)=prepare_signal_transition(&mut inner,&candidate,99.0,20_000,30_000);assert!(!early);let(late,_)=prepare_signal_transition(&mut inner,&candidate,99.0,41_000,30_000);assert!(late);assert_eq!(inner.signal_history.back().unwrap().id,"old");}
#[test]fn signal_identity_is_stable_for_same_entry_evidence(){let first=stable_signal_id("BTCUSDT","1",1_700_000_000_000,&Side::Long);let second=stable_signal_id("BTCUSDT","1",1_700_000_000_000,&Side::Long);let opposite=stable_signal_id("BTCUSDT","1",1_700_000_000_000,&Side::Short);assert_eq!(first,second);assert_ne!(first,opposite);assert_eq!(first,"BTCUSDT:1:1700000000000:long");}
#[test]fn normalized_flow_is_bounded(){assert_eq!(normalized_flow(&[1.0,1.0,-1.0]),1.0/3.0);assert_eq!(normalized_flow(&[]),0.0);}
#[test]fn trade_metrics_detect_large_buy_pressure(){let now=10_000;let mut samples=VecDeque::new();for(offset,notional)in[100.0,120.0,130.0,150.0,200.0,2_000.0].into_iter().enumerate(){samples.push_back(crate::state::FlowSample{ts_ms:now-offset as u64*200,signed_qty:1.0,signed_notional:notional});}let metrics=trade_flow_metrics(&samples,now);assert!(metrics.velocity_per_sec>1.0);assert!(metrics.signed_notional>0.0);assert!(metrics.large_trade_imbalance>0.9);}
#[test]fn liquidation_burst_detects_directional_spike(){let now=60_000;let mut samples=VecDeque::new();for ts in(5_000..55_000).step_by(5_000){samples.push_back(crate::state::FlowSample{ts_ms:ts,signed_qty:1.0,signed_notional:100.0});}samples.push_back(crate::state::FlowSample{ts_ms:59_000,signed_qty:-1.0,signed_notional:-5_000.0});assert!(liquidation_burst_5s(&samples,now)< -0.9);}
#[test]fn oi_window_delta_requires_history_and_tracks_change(){let now=120_000;let samples=VecDeque::from([crate::state::OiSample{ts_ms:60_000,value:100.0},crate::state::OiSample{ts_ms:90_000,value:102.0},crate::state::OiSample{ts_ms:120_000,value:105.0}]);let delta=oi_window_delta_pct(&samples,now,60_000);assert!((delta-5.0).abs()<1e-9);}}

#[cfg(test)]mod lifecycle_tests{use super::*;fn fixture(tf:&str,side:Side)->TradeSignal{let long=matches!(side,Side::Long);TradeSignal{id:tf.into(),symbol:"BTCUSDT".into(),timeframe:tf.into(),side,status:SignalStatus::Active,created_at_ms:1_000,entry_low:100.0,entry_high:100.0,stop_loss:if long{90.0}else{110.0},tp1:if long{114.0}else{86.0},tp2:if long{125.0}else{75.0},risk_reward_tp2:2.5,confidence:0.8,calibrated:false,invalidation:String::new(),reasons:vec![],last_event_ms:1_000,observed_exit_price:None}}
#[test]fn independent_timeframes_and_duplicate_touches(){let mut inner=InternalState::new();inner.signals.insert("1".into(),fixture("1",Side::Long));inner.signals.insert("5".into(),fixture("5",Side::Short));assert!(observe_price(&mut inner,130.0,999).is_empty());let events=observe_price(&mut inner,126.0,2_000);assert_eq!(events.len(),3);assert!(events.iter().any(|e|e.event_type=="tp1"));assert!(events.iter().any(|e|e.event_type=="tp2"));assert!(events.iter().any(|e|e.event_type=="stop_loss"));assert!(observe_price(&mut inner,126.0,3_000).is_empty());for e in events{assert_eq!(crate::flags::from_event(&e).unwrap().price,126.0);}}
#[test]fn short_targets_and_deadline(){let mut inner=InternalState::new();inner.signals.insert("3".into(),fixture("3",Side::Short));assert_eq!(observe_price(&mut inner,85.0,2_000)[0].event_type,"tp1");assert_eq!(observe_price(&mut inner,74.0,3_000)[0].event_type,"tp2");inner.signals.insert("1".into(),fixture("1",Side::Long));assert_eq!(observe_price(&mut inner,100.0,1_000+hold_ms("1")+1)[0].event_type,"expired");}}

#[cfg(test)]mod tick_tests{use super::*;#[test]fn meme_and_large_price_levels_obey_ticks(){for(price,tick,risk)in[(0.00001234,0.00000001,0.00000037),(85342.1,0.1,142.31)]{for direction in[1.0,-1.0]{let(s,t1,t2)=rounded_levels(price,direction,risk,2.5,tick).unwrap();for p in[s,t1,t2]{assert!((p/tick-(p/tick).round()).abs()<1e-5);}assert!(direction*(price-s)>0.0);assert!(direction*(t2-t1)>0.0);assert!((t2-price).abs()/(s-price).abs()>=2.5-1e-8);}}assert!(rounded_levels(1.0,1.0,2.0,2.5,0.1).is_none());}}
