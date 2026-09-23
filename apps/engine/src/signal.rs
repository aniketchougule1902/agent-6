use crate::{
    config::Config,
    runtime,
    state::{now_ms, AppState, InternalState},
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
        // Deadlines still expire during outages; no invented target/stop hit.
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
            let biases: Vec<_> = analyses.iter().map(|a| (a.timeframe.clone(), a.bias.clone())).collect();
            for analysis in &mut analyses {
                let direction = if analysis.bias == "long" {1.0} else {-1.0};
                let flow = (features.trade_flow_imbalance + features.book_imbalance_top5) / 2.0;
                analysis.quality = (0.8 * analysis.quality + 0.2 * (0.5 + 0.5 * direction * flow)).clamp(0.0,1.0);
                if stale { analysis.blockers.push("Live feed is stale".into()); }
                if features.spread_bps > config.max_spread_bps {analysis.blockers.push("Spread above limit".into());}
                if direction * flow < -0.15 {analysis.blockers.push("Order flow opposes the setup".into());}
                let higher = match analysis.timeframe.as_str() {"1"=>"3","3"=>"5","5"=>"15",_=>"5"};
                if !biases.iter().any(|(tf,bias)| tf==higher && bias==&analysis.bias && bias!="neutral") {analysis.blockers.push("Companion timeframe does not confirm".into());}
                if (features.last_price-analysis.close).abs()>analysis.atr14*0.75 {analysis.blockers.push("Live price moved too far from the closed candle".into());}
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
