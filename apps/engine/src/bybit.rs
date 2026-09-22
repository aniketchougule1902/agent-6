use crate::{
    state::{now_ms, AppState, FlowSample},
    types::{AlertKind, Candle, EngineEvent},
};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

pub async fn backfill(state: &AppState) -> Result<()> {
    let client = reqwest::Client::new();
    for interval in ["1", "3", "5", "15"] {
        let url = format!("{}/v5/market/kline", state.config.rest_base_url());
        let response: Value = client
            .get(&url)
            .query(&[
                ("category", "linear"),
                ("symbol", state.config.symbol.as_str()),
                ("interval", interval),
                ("limit", "500"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

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

        let mut inner = state.inner.write();
        for candle in candles {
            inner.upsert_candle(candle);
        }
    }
    info!("historical backfill complete");
    Ok(())
}

pub async fn run_forever(state: AppState) {
    loop {
        match run_session(&state).await {
            Ok(()) => warn!("Bybit stream ended; reconnecting"),
            Err(error) => warn!(?error, "Bybit stream error; reconnecting"),
        }

        {
            let mut inner = state.inner.write();
            inner.connected = false;
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

async fn run_session(state: &AppState) -> Result<()> {
    let (stream, _) = connect_async(state.config.public_ws_url()).await?;
    let (mut write, mut read) = stream.split();

    let symbol = &state.config.symbol;
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
        let mut inner = state.inner.write();
        inner.connected = true;
        inner.updated_at_ms = now_ms();
    }
    state.publish(EngineEvent {
        ts_ms: now_ms(),
        event_type: "feed_reconnected".into(),
        alert: Some(AlertKind::FeedReconnected),
        message: "Bybit public market feed connected.".into(),
        signal: None,
    });

    info!(symbol = %state.config.symbol, "Bybit live feed connected");
    let mut heartbeat = time::interval(Duration::from_secs(20));

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                write.send(Message::Text(json!({"op":"ping"}).to_string().into())).await?;
            }
            maybe_message = read.next() => {
                let message = maybe_message.context("Bybit websocket closed")??;
                match message {
                    Message::Text(text) => handle_message(state, &text),
                    Message::Ping(payload) => write.send(Message::Pong(payload)).await?,
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn handle_message(state: &AppState, text: &str) {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(topic) = root.get("topic").and_then(Value::as_str) else {
        return;
    };

    let now = now_ms();
    let mut inner = state.inner.write();
    inner.updated_at_ms = now;

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
                let sign = if trade.get("S").and_then(Value::as_str) == Some("Buy") { 1.0 } else { -1.0 };
                inner.last_price = trade.get("p").and_then(parse_f64).or(inner.last_price);
                inner.push_trade_flow(FlowSample {
                    ts_ms: trade.get("T").and_then(parse_u64).unwrap_or(now),
                    signed_qty: sign * qty,
                });
            }
        }
    } else if topic.starts_with("orderbook.") {
        if let Some(data) = root.get("data") {
            let bids = data.get("b").and_then(Value::as_array);
            let asks = data.get("a").and_then(Value::as_array);
            let bid_volume = side_volume(bids);
            let ask_volume = side_volume(asks);
            let total = bid_volume + ask_volume;
            if total > 0.0 {
                inner.book_imbalance = (bid_volume - ask_volume) / total;
            }
            if let Some(price) = best_price(bids) {
                inner.bid = Some(price);
            }
            if let Some(price) = best_price(asks) {
                inner.ask = Some(price);
            }
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
                }
            }
            inner.funding_rate = data.get("fundingRate").and_then(parse_f64).or(inner.funding_rate);
            inner.bid = data.get("bid1Price").and_then(parse_f64).or(inner.bid);
            inner.ask = data.get("ask1Price").and_then(parse_f64).or(inner.ask);
        }
    } else if topic.starts_with("allLiquidation.") {
        if let Some(data) = root.get("data").and_then(Value::as_array) {
            for liq in data {
                let qty = liq.get("v").and_then(parse_f64).unwrap_or_default();
                // Bybit documents S=Buy as a long-position liquidation.
                let sign = if liq.get("S").and_then(Value::as_str) == Some("Buy") { -1.0 } else { 1.0 };
                inner.push_liquidation_flow(FlowSample {
                    ts_ms: liq.get("T").and_then(parse_u64).unwrap_or(now),
                    signed_qty: sign * qty,
                });
            }
        }
    }

    inner.prune_flows(now);
}

fn side_volume(levels: Option<&Vec<Value>>) -> f64 {
    levels
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .filter_map(|x| x.get(1))
        .filter_map(parse_f64)
        .sum()
}

fn best_price(levels: Option<&Vec<Value>>) -> Option<f64> {
    levels
        .and_then(|x| x.first())
        .and_then(Value::as_array)
        .and_then(|x| x.first())
        .and_then(parse_f64)
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
