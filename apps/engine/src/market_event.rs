use crate::{
    microstructure::depth_dynamics,
    replay::NormalizedMarketEvent,
    signal,
    state::{AppState, FlowSample, InternalState, OiSample},
    types::{Candle, EngineEvent},
};
use anyhow::{bail, ensure, Result};

/// Apply an already-normalized, already-admitted market-event batch to the same
/// strongly typed state used by the live signal engine.
///
/// `processing_now_ms` is local wall time in live mode and the deterministic
/// replay clock in replay mode. Exchange timestamps remain on each event.
pub fn apply_batch(
    state: &AppState,
    expected_symbol: &str,
    events: &[NormalizedMarketEvent],
    processing_now_ms: u64,
) -> Result<()> {
    let lifecycle_events = {
        let mut inner = state.inner.write();
        apply_batch_to_internal(
            &mut inner,
            expected_symbol,
            events,
            processing_now_ms,
            state.config.stale_feed_ms,
        )?
    };
    for event in lifecycle_events {
        state.publish(event);
    }
    Ok(())
}

/// Pure state-mutation boundary shared by live ingestion and deterministic replay.
/// Validation happens before mutation so a malformed batch is zero-state-mutation.
pub fn validate_batch(events: &[NormalizedMarketEvent], expected_symbol: &str) -> Result<()> {
    ensure!(!events.is_empty(), "normalized market-event batch is empty");
    for event in events {
        validate_event(event, expected_symbol)?;
    }
    Ok(())
}

pub(crate) fn apply_batch_to_internal(
    inner: &mut InternalState,
    expected_symbol: &str,
    events: &[NormalizedMarketEvent],
    processing_now_ms: u64,
    stale_feed_ms: u64,
) -> Result<Vec<EngineEvent>> {
    validate_batch(events, expected_symbol)?;

    let mut lifecycle_events = Vec::new();
    for event in events {
        match event {
            NormalizedMarketEvent::Kline {
                interval,
                start_ms,
                end_ms,
                open,
                high,
                low,
                close,
                volume,
                turnover,
                confirmed,
                ..
            } => {
                inner.last_price = Some(*close);
                inner.upsert_candle(Candle {
                    start_ms: *start_ms,
                    end_ms: *end_ms,
                    interval: interval.clone(),
                    open: *open,
                    high: *high,
                    low: *low,
                    close: *close,
                    volume: *volume,
                    turnover: *turnover,
                    confirmed: *confirmed,
                });
            }
            NormalizedMarketEvent::Trade {
                ts_ms,
                price,
                qty,
                side,
                ..
            } => {
                inner.last_price = Some(*price);
                if processing_now_ms.abs_diff(*ts_ms) <= stale_feed_ms {
                    lifecycle_events.extend(signal::observe_price(inner, *price, *ts_ms));
                }
                let sign = if side == "buy" { 1.0 } else { -1.0 };
                inner.push_trade_flow(FlowSample {
                    ts_ms: *ts_ms,
                    signed_qty: sign * *qty,
                    signed_notional: sign * *qty * *price,
                });
            }
            NormalizedMarketEvent::OrderBook {
                ts_ms,
                update_id,
                seq,
                snapshot,
                bids,
                asks,
                ..
            } => {
                if *snapshot || *update_id == 1 {
                    inner.orderbook_bids.clear();
                    inner.orderbook_asks.clear();
                    inner.previous_bid_depth = 0.0;
                    inner.previous_ask_depth = 0.0;
                } else {
                    if *seq > 0 && inner.orderbook_seq > 0 && *seq <= inner.orderbook_seq {
                        bail!("non-monotonic order-book seq reached state applier");
                    }
                    if *update_id > 0
                        && inner.orderbook_update_id > 0
                        && *update_id <= inner.orderbook_update_id
                    {
                        bail!("non-monotonic order-book update id reached state applier");
                    }
                }
                apply_levels(&mut inner.orderbook_bids, bids);
                apply_levels(&mut inner.orderbook_asks, asks);
                inner.orderbook_update_id = *update_id;
                inner.orderbook_seq = *seq;
                inner.orderbook_event_ms = *ts_ms;
                recalculate_book_metrics(inner);
            }
            NormalizedMarketEvent::Ticker {
                ts_ms,
                last_price,
                mark_price,
                index_price,
                open_interest,
                funding_rate,
                ..
            } => {
                inner.last_price = last_price.or(inner.last_price);
                inner.mark_price = mark_price.or(inner.mark_price);
                inner.index_price = index_price.or(inner.index_price);
                if let Some(oi) = open_interest {
                    if inner.open_interest != Some(*oi) {
                        inner.previous_open_interest = inner.open_interest;
                        inner.open_interest = Some(*oi);
                        inner.push_oi_sample(OiSample {
                            ts_ms: *ts_ms,
                            value: *oi,
                        });
                    }
                }
                inner.funding_rate = funding_rate.or(inner.funding_rate);
            }
            NormalizedMarketEvent::Liquidation {
                ts_ms,
                price,
                qty,
                side,
                ..
            } => {
                // Bybit liquidation side is the liquidated position side. A Sell
                // liquidation is buy pressure and a Buy liquidation is sell pressure.
                let sign = if side == "buy" { -1.0 } else { 1.0 };
                inner.push_liquidation_flow(FlowSample {
                    ts_ms: *ts_ms,
                    signed_qty: sign * *qty,
                    signed_notional: sign * *qty * *price,
                });
            }
        }
    }

    if !events.is_empty() {
        inner.updated_at_ms = processing_now_ms;
        inner.last_market_event_ms = processing_now_ms;
        inner.prune_flows(processing_now_ms);
    }
    Ok(lifecycle_events)
}

fn validate_event(event: &NormalizedMarketEvent, expected_symbol: &str) -> Result<()> {
    let finite_positive = |value: f64, name: &str| -> Result<()> {
        ensure!(value.is_finite() && value > 0.0, "{name} must be finite and positive");
        Ok(())
    };
    let finite_non_negative = |value: f64, name: &str| -> Result<()> {
        ensure!(value.is_finite() && value >= 0.0, "{name} must be finite and non-negative");
        Ok(())
    };
    let symbol = match event {
        NormalizedMarketEvent::Trade { symbol, .. }
        | NormalizedMarketEvent::OrderBook { symbol, .. }
        | NormalizedMarketEvent::Ticker { symbol, .. }
        | NormalizedMarketEvent::Liquidation { symbol, .. }
        | NormalizedMarketEvent::Kline { symbol, .. } => symbol,
    };
    ensure!(symbol == expected_symbol, "event symbol {symbol} does not match active symbol {expected_symbol}");

    match event {
        NormalizedMarketEvent::Trade { ts_ms, price, qty, side, .. }
        | NormalizedMarketEvent::Liquidation { ts_ms, price, qty, side, .. } => {
            ensure!(*ts_ms > 0, "trade/liquidation timestamp must be positive");
            finite_positive(*price, "price")?;
            finite_non_negative(*qty, "quantity")?;
            ensure!(side == "buy" || side == "sell", "side must be buy or sell");
        }
        NormalizedMarketEvent::OrderBook {
            ts_ms,
            bids,
            asks,
            ..
        } => {
            ensure!(*ts_ms > 0, "order-book timestamp must be positive");
            for (price, qty) in bids.iter().chain(asks.iter()) {
                finite_positive(*price, "book price")?;
                finite_non_negative(*qty, "book quantity")?;
            }
        }
        NormalizedMarketEvent::Ticker {
            ts_ms,
            last_price,
            mark_price,
            index_price,
            open_interest,
            funding_rate,
            ..
        } => {
            ensure!(*ts_ms > 0, "ticker timestamp must be positive");
            for (name, value) in [
                ("last price", *last_price),
                ("mark price", *mark_price),
                ("index price", *index_price),
            ] {
                if let Some(value) = value {
                    finite_positive(value, name)?;
                }
            }
            if let Some(value) = open_interest {
                finite_non_negative(*value, "open interest")?;
            }
            if let Some(value) = funding_rate {
                ensure!(value.is_finite(), "funding rate must be finite");
            }
        }
        NormalizedMarketEvent::Kline {
            ts_ms,
            interval,
            start_ms,
            end_ms,
            open,
            high,
            low,
            close,
            volume,
            turnover,
            ..
        } => {
            ensure!(*ts_ms > 0 && *start_ms > 0 && *end_ms >= *start_ms, "invalid kline timestamps");
            ensure!(matches!(interval.as_str(), "1" | "3" | "5" | "15"), "unsupported kline interval");
            for (name, value) in [
                ("open", *open),
                ("high", *high),
                ("low", *low),
                ("close", *close),
            ] {
                finite_positive(value, name)?;
            }
            finite_non_negative(*volume, "volume")?;
            finite_non_negative(*turnover, "turnover")?;
            ensure!(*high >= open.max(*close) && *low <= open.min(*close) && *high >= *low, "invalid OHLC geometry");
        }
    }
    Ok(())
}

fn apply_levels(
    book: &mut std::collections::HashMap<String, (f64, f64)>,
    levels: &[(f64, f64)],
) {
    for (price, size) in levels {
        let key = price.to_string();
        if *size == 0.0 {
            book.remove(&key);
        } else {
            book.insert(key, (*price, *size));
        }
    }
}

fn recalculate_book_metrics(inner: &mut InternalState) {
    let mut bids: Vec<(f64, f64)> = inner.orderbook_bids.values().copied().collect();
    let mut asks: Vec<(f64, f64)> = inner.orderbook_asks.values().copied().collect();
    bids.sort_by(|a, b| b.0.total_cmp(&a.0));
    asks.sort_by(|a, b| a.0.total_cmp(&b.0));

    inner.bid = bids.first().map(|x| x.0);
    inner.ask = asks.first().map(|x| x.0);

    let total_bid: f64 = bids.iter().map(|x| x.1).sum();
    let total_ask: f64 = asks.iter().map(|x| x.1).sum();
    inner.book_imbalance = imbalance(total_bid, total_ask);

    let dynamics = depth_dynamics(
        &bids,
        &asks,
        inner.previous_bid_depth,
        inner.previous_ask_depth,
    );
    inner.bid_depth_slope = dynamics.bid_slope;
    inner.ask_depth_slope = dynamics.ask_slope;
    inner.depth_pressure = dynamics.pressure;
    inner.previous_bid_depth = total_bid;
    inner.previous_ask_depth = total_ask;

    let top5_bid: f64 = bids.iter().take(5).map(|x| x.1).sum();
    let top5_ask: f64 = asks.iter().take(5).map(|x| x.1).sum();
    inner.book_imbalance_top5 = imbalance(top5_bid, top5_ask);

    inner.microprice_bps = match (bids.first(), asks.first()) {
        (Some((bid_price, bid_size)), Some((ask_price, ask_size)))
            if *bid_size + *ask_size > 0.0 =>
        {
            let mid = (bid_price + ask_price) / 2.0;
            let micro =
                (ask_price * bid_size + bid_price * ask_size) / (bid_size + ask_size);
            if mid > 0.0 {
                (micro - mid) / mid * 10_000.0
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
}

fn imbalance(bid: f64, ask: f64) -> f64 {
    let total = bid + ask;
    if total <= f64::EPSILON {
        0.0
    } else {
        ((bid - ask) / total).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::DeterministicReplay;

    fn kline(ts: u64, interval: &str, close: f64) -> NormalizedMarketEvent {
        NormalizedMarketEvent::Kline {
            ts_ms: ts + 59_000,
            symbol: "BTCUSDT".into(),
            interval: interval.into(),
            start_ms: ts,
            end_ms: ts + 59_999,
            open: close - 0.5,
            high: close + 1.0,
            low: close - 1.0,
            close,
            volume: 10.0,
            turnover: close * 10.0,
            confirmed: true,
        }
    }

    #[test]
    fn typed_batch_reconstructs_book_and_candle_without_raw_json() {
        let events = vec![
            NormalizedMarketEvent::OrderBook {
                ts_ms: 1_000,
                symbol: "BTCUSDT".into(),
                update_id: 1,
                seq: 10,
                snapshot: true,
                bids: vec![(100.0, 2.0), (99.0, 1.0)],
                asks: vec![(101.0, 3.0)],
            },
            NormalizedMarketEvent::OrderBook {
                ts_ms: 1_001,
                symbol: "BTCUSDT".into(),
                update_id: 2,
                seq: 11,
                snapshot: false,
                bids: vec![(100.0, 0.0), (99.5, 4.0)],
                asks: vec![],
            },
            kline(60_000, "1", 100.5),
        ];
        let mut inner = InternalState::new();
        let emitted = apply_batch_to_internal(&mut inner, "BTCUSDT", &events, 120_000, 10_000).unwrap();
        assert!(emitted.is_empty());
        assert_eq!(inner.orderbook_seq, 11);
        assert_eq!(inner.bid, Some(99.5));
        assert_eq!(inner.ask, Some(101.0));
        assert_eq!(inner.bars["1"].back().unwrap().start_ms, 60_000);
        assert_eq!(inner.bars["1"].back().unwrap().close, 100.5);
    }

    #[test]
    fn deterministic_replay_produces_identical_typed_state() {
        let events = vec![
            NormalizedMarketEvent::OrderBook {
                ts_ms: 1_000,
                symbol: "BTCUSDT".into(),
                update_id: 1,
                seq: 10,
                snapshot: true,
                bids: vec![(100.0, 2.0)],
                asks: vec![(101.0, 3.0)],
            },
            NormalizedMarketEvent::Trade {
                ts_ms: 1_100,
                symbol: "BTCUSDT".into(),
                price: 100.5,
                qty: 0.25,
                side: "buy".into(),
            },
            kline(60_000, "1", 100.75),
        ];
        let mut first = InternalState::new();
        for frame in DeterministicReplay::try_new(events.clone()).unwrap() {
            apply_batch_to_internal(
                &mut first,
                "BTCUSDT",
                &[frame.event],
                frame.replay_ts_ms,
                10_000,
            )
            .unwrap();
        }
        let first_book = (first.bid, first.ask, first.orderbook_seq);
        let first_price = first.last_price;
        let first_candles: Vec<_> = first.bars["1"].iter().map(|c| (c.start_ms, c.close)).collect();

        let mut second = InternalState::new();
        for frame in DeterministicReplay::try_new(events).unwrap() {
            apply_batch_to_internal(
                &mut second,
                "BTCUSDT",
                &[frame.event],
                frame.replay_ts_ms,
                10_000,
            )
            .unwrap();
        }
        assert_eq!(first_book, (second.bid, second.ask, second.orderbook_seq));
        assert_eq!(first_price, second.last_price);
        assert_eq!(first_candles, second.bars["1"].iter().map(|c| (c.start_ms, c.close)).collect::<Vec<_>>());
        assert_eq!(first.trade_flow.len(), second.trade_flow.len());
    }

    #[test]
    fn malformed_batch_is_zero_state_mutation() {
        let mut inner = InternalState::new();
        let bad = vec![
            NormalizedMarketEvent::Trade {
                ts_ms: 1_000,
                symbol: "BTCUSDT".into(),
                price: 100.0,
                qty: 1.0,
                side: "buy".into(),
            },
            NormalizedMarketEvent::Trade {
                ts_ms: 1_001,
                symbol: "ETHUSDT".into(),
                price: 2_000.0,
                qty: 1.0,
                side: "buy".into(),
            },
        ];
        assert!(apply_batch_to_internal(&mut inner, "BTCUSDT", &bad, 1_001, 10_000).is_err());
        assert!(inner.last_price.is_none());
        assert!(inner.trade_flow.is_empty());
    }
}
