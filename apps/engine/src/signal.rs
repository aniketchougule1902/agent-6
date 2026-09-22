use crate::{
    state::{now_ms, AppState, InternalState},
    types::{
        AlertKind, Candle, EngineEvent, FeatureSnapshot, MarketRegime, Side, SignalStatus,
        TradeSignal,
    },
};
use std::time::Duration;
use tokio::time;
use uuid::Uuid;

pub async fn run_loop(state: AppState) {
    let mut ticker = time::interval(Duration::from_millis(400));
    loop {
        ticker.tick().await;
        let now = now_ms();
        let mut events = Vec::new();

        {
            let mut inner = state.inner.write();
            inner.prune_flows(now);

            let stale_now = inner.connected
                && now.saturating_sub(inner.last_market_event_ms) > state.config.stale_feed_ms;
            if stale_now != inner.feed_stale {
                inner.feed_stale = stale_now;
                events.push(EngineEvent {
                    ts_ms: now,
                    event_type: if stale_now { "feed_stale" } else { "feed_recovered" }.into(),
                    alert: Some(if stale_now {
                        AlertKind::FeedStale
                    } else {
                        AlertKind::FeedRecovered
                    }),
                    message: if stale_now {
                        format!(
                            "Market feed stale for {} ms; new signals suspended.",
                            now.saturating_sub(inner.last_market_event_ms)
                        )
                    } else {
                        "Fresh market events resumed; signal generation re-enabled.".into()
                    },
                    signal: inner.active_signal.clone(),
                });
            }

            if let Some(features) = compute_features(&inner, now) {
                inner.features = Some(features.clone());

                if !inner.feed_stale {
                    events.extend(update_signal_lifecycle(&mut inner, now));

                    let terminal = inner.active_signal.as_ref().is_some_and(|signal| {
                        matches!(
                            signal.status,
                            SignalStatus::Tp2Hit | SignalStatus::StopLossHit | SignalStatus::Expired
                        )
                    });
                    if terminal
                        && now.saturating_sub(inner.last_signal_at_ms)
                            > state.config.signal_cooldown_secs * 1000
                    {
                        inner.active_signal = None;
                    }

                    if inner.active_signal.is_none()
                        && now.saturating_sub(inner.last_signal_at_ms)
                            > state.config.signal_cooldown_secs * 1000
                    {
                        if let Some(signal) = build_signal(&state, &features, now) {
                            inner.last_signal_at_ms = now;
                            inner.active_signal = Some(signal.clone());
                            events.push(EngineEvent {
                                ts_ms: now,
                                event_type: "signal".into(),
                                alert: Some(AlertKind::Signal),
                                message: format!(
                                    "{} {:?} setup at {:.4} (quality {:.1}%)",
                                    signal.symbol,
                                    signal.side,
                                    (signal.entry_low + signal.entry_high) / 2.0,
                                    signal.confidence * 100.0
                                ),
                                signal: Some(signal),
                            });
                        }
                    }
                }
            }
            inner.updated_at_ms = now;
        }

        for event in events {
            state.publish(event);
        }
    }
}

fn compute_features(s: &InternalState, now: u64) -> Option<FeatureSnapshot> {
    let price = s.last_price?;
    let bars_1m = s.bars.get("1")?;
    let bars_5m = s.bars.get("5")?;
    let bars_15m = s.bars.get("15")?;
    if bars_1m.len() < 30 || bars_5m.len() < 22 || bars_15m.len() < 22 {
        return None;
    }

    let atr = atr(bars_1m, 14)?;
    let vwap = vwap(bars_1m, 20)?;
    let momentum = return_bps(bars_1m, 5).unwrap_or_default();
    let trend_5m = ema_spread_bps(bars_5m, 8, 21).unwrap_or_default();
    let trend_15m = ema_spread_bps(bars_15m, 8, 21).unwrap_or_default();
    let spread_bps = match (s.bid, s.ask) {
        (Some(bid), Some(ask)) if bid > 0.0 && ask >= bid => ((ask - bid) / ((ask + bid) / 2.0)) * 10_000.0,
        _ => 0.0,
    };

    let flow = normalized_flow(&s.trade_flow.iter().map(|x| x.signed_qty).collect::<Vec<_>>());
    let liq = normalized_flow(&s.liquidation_flow.iter().map(|x| x.signed_qty).collect::<Vec<_>>());
    let oi_delta_pct = match (s.previous_open_interest, s.open_interest) {
        (Some(prev), Some(current)) if prev.abs() > f64::EPSILON => (current - prev) / prev * 100.0,
        _ => 0.0,
    };

    let atr_bps = atr / price * 10_000.0;
    let regime = if atr_bps > 35.0 {
        MarketRegime::HighVolatility
    } else if trend_15m > 4.0 && trend_5m > 2.0 {
        MarketRegime::TrendingUp
    } else if trend_15m < -4.0 && trend_5m < -2.0 {
        MarketRegime::TrendingDown
    } else {
        MarketRegime::Range
    };

    let directional_edge =
        signed_unit(trend_15m / 12.0) * 0.08
        + signed_unit(trend_5m / 10.0) * 0.07
        + signed_unit(momentum / 25.0) * 0.07
        + signed_unit(((price - vwap) / price * 10_000.0) / 12.0) * 0.07
        + s.book_imbalance.clamp(-1.0, 1.0) * 0.07
        + s.book_imbalance_top5.clamp(-1.0, 1.0) * 0.08
        + signed_unit(s.microprice_bps / 1.5) * 0.05
        + flow * 0.14
        + liq * 0.05
        + signed_unit(oi_delta_pct / 0.08) * signed_unit(momentum / 20.0).abs() * 0.04;

    let crowding_penalty = s
        .funding_rate
        .map(|f| (f.abs() / 0.001).clamp(0.0, 1.0) * 0.03)
        .unwrap_or_default();

    let long_score = (0.5 + directional_edge - crowding_penalty).clamp(0.0, 1.0);
    let short_score = (0.5 - directional_edge - crowding_penalty).clamp(0.0, 1.0);

    Some(FeatureSnapshot {
        ts_ms: now,
        feed_age_ms: now.saturating_sub(s.last_market_event_ms),
        orderbook_age_ms: now.saturating_sub(s.orderbook_event_ms),
        last_price: price,
        spread_bps,
        atr_14: atr,
        vwap_20: vwap,
        momentum_1m_bps: momentum,
        trend_5m_bps: trend_5m,
        trend_15m_bps: trend_15m,
        book_imbalance: s.book_imbalance,
        book_imbalance_top5: s.book_imbalance_top5,
        microprice_bps: s.microprice_bps,
        trade_flow_imbalance: flow,
        liquidation_pressure: liq,
        open_interest: s.open_interest,
        open_interest_delta_pct: oi_delta_pct,
        funding_rate: s.funding_rate,
        regime,
        long_score,
        short_score,
    })
}

fn build_signal(state: &AppState, f: &FeatureSnapshot, now: u64) -> Option<TradeSignal> {
    if f.feed_age_ms > state.config.stale_feed_ms
        || f.orderbook_age_ms > state.config.stale_feed_ms
        || f.spread_bps > state.config.max_spread_bps
    {
        return None;
    }

    let (side, confidence) = if f.long_score >= f.short_score {
        (Side::Long, f.long_score)
    } else {
        (Side::Short, f.short_score)
    };
    if confidence < state.config.min_signal_score {
        return None;
    }

    let price = f.last_price;
    let risk = (f.atr_14 * 1.20).max(price * 0.0010);
    let rr2 = state.config.min_rr.max(2.2);
    let entry_half_width = (f.atr_14 * 0.08).max(price * 0.00005);

    let (stop_loss, tp1, tp2, invalidation) = match side {
        Side::Long => (
            price - risk,
            price + risk * 1.4,
            price + risk * rr2,
            format!("invalidate on live trade <= {:.4}", price - risk),
        ),
        Side::Short => (
            price + risk,
            price - risk * 1.4,
            price - risk * rr2,
            format!("invalidate on live trade >= {:.4}", price + risk),
        ),
    };

    let mut reasons = vec![
        format!("15m trend {:.1} bps", f.trend_15m_bps),
        format!("5m trend {:.1} bps", f.trend_5m_bps),
        format!("L50 imbalance {:.2}", f.book_imbalance),
        format!("top5 imbalance {:.2}", f.book_imbalance_top5),
        format!("microprice {:.2} bps", f.microprice_bps),
        format!("trade-flow imbalance {:.2}", f.trade_flow_imbalance),
        format!("spread {:.2} bps", f.spread_bps),
    ];
    if f.open_interest_delta_pct.abs() > 0.01 {
        reasons.push(format!("OI delta {:.3}%", f.open_interest_delta_pct));
    }

    Some(TradeSignal {
        id: Uuid::new_v4().to_string(),
        symbol: state.config.symbol.clone(),
        timeframe: state.config.timeframe.clone(),
        side,
        status: SignalStatus::Active,
        created_at_ms: now,
        entry_low: price - entry_half_width,
        entry_high: price + entry_half_width,
        stop_loss,
        tp1,
        tp2,
        risk_reward_tp2: rr2,
        confidence,
        calibrated: false,
        invalidation,
        reasons,
    })
}

fn update_signal_lifecycle(inner: &mut InternalState, now: u64) -> Vec<EngineEvent> {
    let Some(price) = inner.last_price else {
        return vec![];
    };
    let Some(signal) = inner.active_signal.as_mut() else {
        return vec![];
    };

    let mut event = None;
    match signal.side {
        Side::Long => {
            if price <= signal.stop_loss
                && matches!(signal.status, SignalStatus::Active | SignalStatus::Tp1Hit)
            {
                signal.status = SignalStatus::StopLossHit;
                inner.last_signal_at_ms = now;
                event = Some(("stop_loss", AlertKind::StopLoss, "Stop-loss hit"));
            } else if price >= signal.tp2
                && matches!(signal.status, SignalStatus::Active | SignalStatus::Tp1Hit)
            {
                signal.status = SignalStatus::Tp2Hit;
                inner.last_signal_at_ms = now;
                event = Some(("tp2", AlertKind::Tp2, "TP2 hit"));
            } else if price >= signal.tp1 && signal.status == SignalStatus::Active {
                signal.status = SignalStatus::Tp1Hit;
                event = Some(("tp1", AlertKind::Tp1, "TP1 hit"));
            }
        }
        Side::Short => {
            if price >= signal.stop_loss
                && matches!(signal.status, SignalStatus::Active | SignalStatus::Tp1Hit)
            {
                signal.status = SignalStatus::StopLossHit;
                inner.last_signal_at_ms = now;
                event = Some(("stop_loss", AlertKind::StopLoss, "Stop-loss hit"));
            } else if price <= signal.tp2
                && matches!(signal.status, SignalStatus::Active | SignalStatus::Tp1Hit)
            {
                signal.status = SignalStatus::Tp2Hit;
                inner.last_signal_at_ms = now;
                event = Some(("tp2", AlertKind::Tp2, "TP2 hit"));
            } else if price <= signal.tp1 && signal.status == SignalStatus::Active {
                signal.status = SignalStatus::Tp1Hit;
                event = Some(("tp1", AlertKind::Tp1, "TP1 hit"));
            }
        }
    }

    event
        .map(|(event_type, alert, label)| {
            vec![EngineEvent {
                ts_ms: now,
                event_type: event_type.into(),
                alert: Some(alert),
                message: format!("{label} at live price {:.4}", price),
                signal: Some(signal.clone()),
            }]
        })
        .unwrap_or_default()
}

fn atr(bars: &std::collections::VecDeque<Candle>, period: usize) -> Option<f64> {
    if bars.len() < period + 1 {
        return None;
    }
    let slice: Vec<_> = bars.iter().rev().take(period + 1).collect();
    let mut sum = 0.0;
    for pair in slice.windows(2) {
        let current = pair[0];
        let previous = pair[1];
        let tr = (current.high - current.low)
            .max((current.high - previous.close).abs())
            .max((current.low - previous.close).abs());
        sum += tr;
    }
    Some(sum / period as f64)
}

fn vwap(bars: &std::collections::VecDeque<Candle>, period: usize) -> Option<f64> {
    let mut pv = 0.0;
    let mut volume = 0.0;
    for bar in bars.iter().rev().take(period) {
        pv += bar.close * bar.volume;
        volume += bar.volume;
    }
    (volume > 0.0).then_some(pv / volume)
}

fn return_bps(bars: &std::collections::VecDeque<Candle>, lookback: usize) -> Option<f64> {
    let current = bars.back()?.close;
    let previous = bars.get(bars.len().checked_sub(lookback + 1)?)?.close;
    Some((current / previous - 1.0) * 10_000.0)
}

fn ema_spread_bps(
    bars: &std::collections::VecDeque<Candle>,
    fast: usize,
    slow: usize,
) -> Option<f64> {
    let closes: Vec<f64> = bars.iter().map(|x| x.close).collect();
    let price = *closes.last()?;
    let fast_ema = ema(&closes, fast)?;
    let slow_ema = ema(&closes, slow)?;
    Some((fast_ema - slow_ema) / price * 10_000.0)
}

fn ema(values: &[f64], period: usize) -> Option<f64> {
    if values.len() < period {
        return None;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut result = values[0];
    for value in &values[1..] {
        result = alpha * *value + (1.0 - alpha) * result;
    }
    Some(result)
}

fn normalized_flow(samples: &[f64]) -> f64 {
    let signed: f64 = samples.iter().sum();
    let absolute: f64 = samples.iter().map(|x| x.abs()).sum();
    if absolute <= f64::EPSILON {
        0.0
    } else {
        (signed / absolute).clamp(-1.0, 1.0)
    }
}

fn signed_unit(value: f64) -> f64 {
    value.tanh()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_flow_is_bounded() {
        assert_eq!(normalized_flow(&[1.0, 1.0, -1.0]), 1.0 / 3.0);
        assert_eq!(normalized_flow(&[]), 0.0);
    }
}
