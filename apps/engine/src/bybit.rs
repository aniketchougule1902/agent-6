use crate::{
    live_recorder::AcceptedMarketRecorder,
    microstructure::depth_dynamics,
    runtime::{self, RuntimeMarketConfig},
    state::{now_ms, AppState, FlowSample, InternalState, OiSample},
    types::{AlertKind, Candle, EngineEvent},
};
use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{collections::HashMap, time::Duration};
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

pub async fn backfill(state: &AppState, symbol: &str) -> Result<()> {
    let client = reqwest::Client::new();
    for interval in ["1", "3", "5", "15"] {
        let endpoints: &[&str] = if state.config.bybit_testnet {
            &["https://api-testnet.bybit.com"]
        } else {
            &["https://api.bybit.com", "https://api.bytick.com"]
        };
        let mut response = None;
        for base in endpoints {
            let url = format!("{base}/v5/market/kline");
            let result = client
                .get(&url)
                .query(&[
                    ("category", "linear"),
                    ("symbol", symbol),
                    ("interval", interval),
                    ("limit", "500"),
                ])
                .send()
                .await
                .and_then(reqwest::Response::error_for_status);
            match result {
                Ok(reply) => {
                    response = Some(reply.json::<Value>().await?);
                    break;
                }
                Err(error) => warn!(?error, %url, "historical endpoint failed; trying next"),
            }
        }
        let response = response.with_context(|| format!("all Bybit historical endpoints failed for {interval}m"))?;
        if response.get("retCode").and_then(Value::as_i64) != Some(0) {
            bail!("Bybit rejected historical {interval}m request: {}", response.get("retMsg").and_then(Value::as_str).unwrap_or("unknown error"));
        }

        let list = response
            .pointer("/result/list")
            .and_then(Value::as_array)
            .with_context(|| format!("missing Bybit kline result for {interval}m"))?;

        let mut candles = Vec::with_capacity(list.len());
        for row in list.iter().rev() {
            if let Some(values) = row.as_array() {
                if values.len() >= 7 {
                    candles.push(Candle {
                        start_ms: parse_u64(&values[0]).unwrap_or_default(),
                        end_ms: 0,
                        interval: interval.into(),
                        open: parse_f64(&values[1]).unwrap_or_default(),
                        high: parse_f64(&values[2]).unwrap_or_default(),
                        low: parse_f64(&values[3]).unwrap_or_default(),
                        close: parse_f64(&values[4]).unwrap_or_default(),
                        volume: parse_f64(&values[5]).unwrap_or_default(),
                        turnover: parse_f64(&values[6]).unwrap_or_default(),
                        confirmed: true,
                    });
                }
            }
        }

        if runtime::current().symbol != symbol {
            bail!("historical backfill canceled after market change");
        }
        let mut inner = state.inner.write();
        for candle in candles {
            inner.upsert_candle(candle);
        }
    }
    info!(symbol, "historical backfill complete");
    Ok(())
}

enum SessionEnd {
    Reconfigure,
    Disconnected,
}

pub async fn run_forever(state: AppState) {
    let mut market_changes = runtime::subscribe();
    let mut loaded_symbol = String::new();
    let mut backfill_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut recorder = match AcceptedMarketRecorder::open(&state.config.market_record_path) {
        Ok(recorder) => recorder,
        Err(error) => {
            warn!(?error, "market recorder unavailable; live feed halted");
            return;
        }
    };

    loop {
        let market = market_changes.borrow().clone();

        if loaded_symbol != market.symbol {
            if let Some(task) = backfill_task.take() {
                task.abort();
            }
            state.inner.write().reset_market();
            let backfill_state = state.clone();
            let backfill_symbol = market.symbol.clone();
            backfill_task = Some(tokio::spawn(async move {
                loop {
                    match backfill(&backfill_state, &backfill_symbol).await {
                        Ok(()) => break,
                        Err(error) => {
                            warn!(?error, symbol = %backfill_symbol, "historical backfill failed; retrying in 15s");
                            time::sleep(Duration::from_secs(15)).await;
                        }
                    }
                }
            }));
            loaded_symbol = market.symbol.clone();
        }

        recorder.reset_symbol(&market.symbol);
        match run_session(&state, &market, &mut market_changes, &mut recorder).await {
            Ok(SessionEnd::Reconfigure) => {
                let next = market_changes.borrow().clone();
                state.inner.write().reset_market();
                state.publish(EngineEvent {
                    ts_ms: now_ms(),
                    event_type: "market_reconfigured".into(),
                    alert: None,
                    message: format!(
                        "Market switched from {} to {}; rebuilding local state.",
                        market.symbol, next.symbol
                    ),
                    signal: None,
                });
                continue;
            }
            Ok(SessionEnd::Disconnected) => warn!("Bybit stream ended; reconnecting"),
            Err(error) => warn!(?error, "Bybit stream error; reconnecting"),
        }

        {
            let mut inner = state.inner.write();
            inner.connected = false;
            inner.feed_stale = true;
            inner.updated_at_ms = now_ms();
        }
        state.publish(EngineEvent {
            ts_ms: now_ms(),
            event_type: "feed_disconnected".into(),
            alert: Some(AlertKind::FeedDisconnected),
            message: "Bybit public market feed disconnected; reconnecting.".into(),
            signal: None,
        });
        time::sleep(Duration::from_secs(2)).await;
    }
}

async fn run_session(
    state: &AppState,
    market: &RuntimeMarketConfig,
    market_changes: &mut tokio::sync::watch::Receiver<RuntimeMarketConfig>,
    recorder: &mut AcceptedMarketRecorder,
) -> Result<SessionEnd> {
    let (stream, _) = connect_async(state.config.public_ws_url()).await?;
    let (mut write, mut read) = stream.split();

    let symbol = &market.symbol;
    let args = vec![
        format!("kline.1.{symbol}"),
        format!("kline.3.{symbol}"),
        format!("kline.5.{symbol}"),
        format!("kline.15.{symbol}"),
        format!("publicTrade.{symbol}"),
        format!("orderbook.50.{symbol}"),
        format!("tickers.{symbol}"),
        format!("allLiquidation.{symbol}"),
    ];

    write
        .send(Message::Text(
            json!({"op":"subscribe","args":args}).to_string().into(),
        ))
        .await?;

    {
        let now = now_ms();
        let mut inner = state.inner.write();
        inner.connected = true;
        inner.feed_stale = true;
        inner.orderbook_bids.clear();
        inner.orderbook_asks.clear();
        inner.orderbook_seq = 0;
        inner.orderbook_update_id = 0;
        inner.orderbook_event_ms = 0;
        inner.bid = None;
        inner.ask = None;
        inner.last_market_event_ms = now;
        inner.updated_at_ms = now;
    }
    state.publish(EngineEvent {
        ts_ms: now_ms(),
        event_type: "feed_reconnected".into(),
        alert: Some(AlertKind::FeedReconnected),
        message: "Bybit public market feed connected.".into(),
        signal: None,
    });

    info!(symbol = %market.symbol, generation = market.generation, "Bybit live feed connected");
    let mut heartbeat = time::interval(Duration::from_secs(20));

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                write.send(Message::Text(json!({"op":"ping"}).to_string().into())).await?;
            }
            changed = market_changes.changed() => {
                changed.context("runtime market configuration channel closed")?;
                let next = market_changes.borrow().clone();
                if next.symbol != market.symbol {
                    return Ok(SessionEnd::Reconfigure);
                }
                state.inner.write().reset_signal_context();
                state.publish(EngineEvent {
                    ts_ms: now_ms(),
                    event_type: "timeframe_changed".into(),
                    alert: None,
                    message: format!("Analysis timeframe changed to {}m.", next.timeframe),
                    signal: None,
                });
            }
            maybe_message = read.next() => {
                let Some(message) = maybe_message else {
                    return Ok(SessionEnd::Disconnected);
                };
                let message = message?;
                match message {
                    Message::Text(text) => {
                        if text.contains("\"topic\"") {
                            let events = crate::bybit_record::normalize_message(&text);
                            if events.is_empty() {
                                warn!("unrecognized market payload ignored");
                                continue;
                            }
                            crate::market_event::validate_batch(&events, &market.symbol)
                                .context("market payload failed typed validation before recording")?;
                            recorder.append_batch(&events)
                                .context("market payload rejected before live-state mutation")?;
                            crate::market_event::apply_batch(
                                state,
                                &market.symbol,
                                &events,
                                now_ms(),
                            ).context("accepted market payload failed typed state application")?;
                        }
                    },
                    Message::Ping(payload) => write.send(Message::Pong(payload)).await?,
                    Message::Close(_) => return Ok(SessionEnd::Disconnected),
                    _ => {}
                }
            }
        }
    }
}

fn handle_message(state: &AppState, text: &str) {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(topic) = root.get("topic").and_then(Value::as_str) else {
        return;
    };
    if topic.rsplit('.').next() != Some(runtime::current().symbol.as_str()) { return; }

    let now = now_ms();
    let mut lifecycle_events = Vec::new();
    let mut inner = state.inner.write();
    inner.updated_at_ms = now;
    inner.last_market_event_ms = now;

    if topic.starts_with("kline.") {
        if let Some(data) = root.get("data").and_then(Value::as_array) {
            for item in data {
                let candle = Candle {
                    start_ms: item.get("start").and_then(parse_u64).unwrap_or_default(),
                    end_ms: item.get("end").and_then(parse_u64).unwrap_or_default(),
                    interval: item.get("interval").and_then(Value::as_str).unwrap_or("1").to_string(),
                    open: item.get("open").and_then(parse_f64).unwrap_or_default(),
                    high: item.get("high").and_then(parse_f64).unwrap_or_default(),
                    low: item.get("low").and_then(parse_f64).unwrap_or_default(),
                    close: item.get("close").and_then(parse_f64).unwrap_or_default(),
                    volume: item.get("volume").and_then(parse_f64).unwrap_or_default(),
                    turnover: item.get("turnover").and_then(parse_f64).unwrap_or_default(),
                    confirmed: item.get("confirm").and_then(Value::as_bool).unwrap_or(false),
                };
                inner.last_price = Some(candle.close);
                inner.upsert_candle(candle);
            }
        }
    } else if topic.starts_with("publicTrade.") {
        if let Some(data) = root.get("data").and_then(Value::as_array) {
            for trade in data {
                let qty = trade.get("v").and_then(parse_f64).unwrap_or_default();
                let price = trade.get("p").and_then(parse_f64).unwrap_or_default();
                let sign = if trade.get("S").and_then(Value::as_str) == Some("Buy") { 1.0 } else { -1.0 };
                if price > 0.0 {
                    inner.last_price = Some(price);
                    let trade_ms = trade.get("T").and_then(parse_u64).unwrap_or(0);
                    if now.abs_diff(trade_ms) <= state.config.stale_feed_ms {
                        lifecycle_events.extend(crate::signal::observe_price(&mut inner, price, trade_ms));
                    }
                }
                inner.push_trade_flow(FlowSample {
                    ts_ms: trade.get("T").and_then(parse_u64).unwrap_or(now),
                    signed_qty: sign * qty,
                    signed_notional: sign * qty * price,
                });
            }
        }
    } else if topic.starts_with("orderbook.") {
        if let Some(data) = root.get("data") {
            let kind = root.get("type").and_then(Value::as_str).unwrap_or("delta");
            let update_id = data.get("u").and_then(Value::as_u64).unwrap_or_default();
            let seq = data.get("seq").and_then(Value::as_u64).unwrap_or_default();
            let cts = data.get("cts").and_then(Value::as_u64).unwrap_or(now);
            let reset = kind == "snapshot" || update_id == 1;

            if reset {
                inner.orderbook_bids.clear();
                inner.orderbook_asks.clear();
                inner.previous_bid_depth = 0.0;
                inner.previous_ask_depth = 0.0;
            } else if seq > 0 && inner.orderbook_seq > 0 && seq <= inner.orderbook_seq {
                return;
            }

            if let Some(bids) = data.get("b").and_then(Value::as_array) {
                apply_levels(&mut inner.orderbook_bids, bids);
            }
            if let Some(asks) = data.get("a").and_then(Value::as_array) {
                apply_levels(&mut inner.orderbook_asks, asks);
            }
            inner.orderbook_update_id = update_id;
            inner.orderbook_seq = seq;
            inner.orderbook_event_ms = cts;
            recalculate_book_metrics(&mut inner);
        }
    } else if topic.starts_with("tickers.") {
        if let Some(data) = root.get("data") {
            let data = if data.is_array() { data.get(0).unwrap_or(data) } else { data };
            inner.last_price = data.get("lastPrice").and_then(parse_f64).or(inner.last_price);
            inner.mark_price = data.get("markPrice").and_then(parse_f64).or(inner.mark_price);
            inner.index_price = data.get("indexPrice").and_then(parse_f64).or(inner.index_price);
            if let Some(oi) = data.get("openInterest").and_then(parse_f64) {
                if inner.open_interest != Some(oi) {
                    inner.previous_open_interest = inner.open_interest;
                    inner.open_interest = Some(oi);
                    inner.push_oi_sample(OiSample { ts_ms: now, value: oi });
                }
            }
            inner.funding_rate = data.get("fundingRate").and_then(parse_f64).or(inner.funding_rate);
        }
    } else if topic.starts_with("allLiquidation.") {
        if let Some(data) = root.get("data").and_then(Value::as_array) {
            for liq in data {
                let qty = liq.get("v").and_then(parse_f64).unwrap_or_default();
                let price = liq.get("p").and_then(parse_f64).unwrap_or_default();
                let sign = if liq.get("S").and_then(Value::as_str) == Some("Buy") { -1.0 } else { 1.0 };
                inner.push_liquidation_flow(FlowSample {
                    ts_ms: liq.get("T").and_then(parse_u64).unwrap_or(now),
                    signed_qty: sign * qty,
                    signed_notional: sign * qty * price,
                });
            }
        }
    }

    inner.prune_flows(now);
    drop(inner);
    for event in lifecycle_events { state.publish(event); }
}

fn apply_levels(book: &mut HashMap<String, (f64, f64)>, levels: &[Value]) {
    for level in levels {
        let Some(values) = level.as_array() else {
            continue;
        };
        let Some(price_text) = values.first().and_then(Value::as_str) else {
            continue;
        };
        let Some(price) = values.first().and_then(parse_f64) else {
            continue;
        };
        let size = values.get(1).and_then(parse_f64).unwrap_or_default();
        if size == 0.0 {
            book.remove(price_text);
        } else {
            book.insert(price_text.to_string(), (price, size));
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
            let micro = (ask_price * bid_size + bid_price * ask_size) / (bid_size + ask_size);
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

fn parse_f64(value: &Value) -> Option<f64> {
    value
        .as_str()
        .and_then(|x| x.parse().ok())
        .or_else(|| value.as_f64())
}

fn parse_u64(value: &Value) -> Option<u64> {
    value
        .as_str()
        .and_then(|x| x.parse().ok())
        .or_else(|| value.as_u64())
}
