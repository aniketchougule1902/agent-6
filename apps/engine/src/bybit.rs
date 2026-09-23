use crate::{
    live_recorder::AcceptedMarketRecorder,
    replay::NormalizedMarketEvent,
    runtime::{self, RuntimeMarketConfig},
    state::{now_ms, AppState},
    types::{AlertKind, Candle, EngineEvent},
};
use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

pub async fn backfill(
    state: &AppState,
    symbol: &str,
    recorder: &Arc<Mutex<AcceptedMarketRecorder>>,
) -> Result<()> {
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

        let fetched_at_ms = now_ms();
        let mut candles = Vec::with_capacity(list.len());
        let mut events = Vec::with_capacity(list.len());
        for row in list.iter().rev() {
            let values = row
                .as_array()
                .with_context(|| format!("malformed Bybit {interval}m kline row"))?;
            if let Some((candle, event)) =
                parse_closed_rest_kline(symbol, interval, values, fetched_at_ms)?
            {
                candles.push(candle);
                events.push(event);
            }
        }

        if runtime::current().symbol != symbol {
            bail!("historical backfill canceled after market change");
        }
        crate::market_event::validate_batch(&events, symbol)
            .with_context(|| format!("invalid normalized {interval}m REST bootstrap batch"))?;
        recorder
            .lock()
            .await
            .append_batch(&events)
            .with_context(|| format!("persist {interval}m REST bootstrap batch"))?;

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
    let mut recorder: Option<Arc<Mutex<AcceptedMarketRecorder>>> = None;

    loop {
        let market = market_changes.borrow().clone();

        if loaded_symbol != market.symbol {
            if let Some(task) = backfill_task.take() {
                task.abort();
                let _ = task.await;
            }
            // Drop the old writer before rotating the active session file. Each
            // symbol/process session remains independently replayable.
            recorder.take();
            let session_label = format!("{}-g{}", market.symbol, market.generation);
            match AcceptedMarketRecorder::start_session(&state.config.market_record_path, &session_label) {
                Ok((next_recorder, archived)) => {
                    if let Some(path) = archived {
                        info!(path = %path.display(), "archived previous normalized market recording");
                    }
                    recorder = Some(Arc::new(Mutex::new(next_recorder)));
                }
                Err(error) => {
                    warn!(?error, "market recorder unavailable; live feed halted");
                    return;
                }
            }
            state.inner.write().reset_market();
            let backfill_state = state.clone();
            let backfill_symbol = market.symbol.clone();
            let backfill_recorder = recorder.as_ref().expect("recorder opened above").clone();
            backfill_task = Some(tokio::spawn(async move {
                loop {
                    match backfill(&backfill_state, &backfill_symbol, &backfill_recorder).await {
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

        let Some(recorder) = recorder.as_ref() else {
            warn!("market recorder session missing; live feed halted");
            return;
        };
        recorder.lock().await.reset_symbol(&market.symbol);
        match run_session(&state, &market, &mut market_changes, recorder).await {
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
    recorder: &Arc<Mutex<AcceptedMarketRecorder>>,
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
                            recorder.lock().await.append_batch(&events)
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


fn parse_closed_rest_kline(
    symbol: &str,
    interval: &str,
    values: &[Value],
    fetched_at_ms: u64,
) -> Result<Option<(Candle, NormalizedMarketEvent)>> {
    if values.len() < 7 {
        bail!("Bybit REST kline row has fewer than seven values");
    }
    let duration_ms = interval
        .parse::<u64>()
        .with_context(|| format!("invalid kline interval {interval}"))?
        .checked_mul(60_000)
        .context("kline interval overflow")?;
    let start_ms = parse_u64(&values[0]).context("missing kline start")?;
    let close_boundary_ms = start_ms
        .checked_add(duration_ms)
        .context("kline close boundary overflow")?;
    if close_boundary_ms > fetched_at_ms {
        return Ok(None);
    }
    let end_ms = close_boundary_ms.saturating_sub(1);
    let open = parse_f64(&values[1]).context("missing kline open")?;
    let high = parse_f64(&values[2]).context("missing kline high")?;
    let low = parse_f64(&values[3]).context("missing kline low")?;
    let close = parse_f64(&values[4]).context("missing kline close")?;
    let volume = parse_f64(&values[5]).context("missing kline volume")?;
    let turnover = parse_f64(&values[6]).context("missing kline turnover")?;

    let candle = Candle {
        start_ms,
        end_ms,
        interval: interval.into(),
        open,
        high,
        low,
        close,
        volume,
        turnover,
        confirmed: true,
    };
    let event = NormalizedMarketEvent::Kline {
        ts_ms: end_ms,
        symbol: symbol.into(),
        interval: interval.into(),
        start_ms,
        end_ms,
        open,
        high,
        low,
        close,
        volume,
        turnover,
        confirmed: true,
    };
    Ok(Some((candle, event)))
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

#[cfg(test)]
mod rest_backfill_tests {
    use super::*;

    fn row(start_ms: u64) -> Vec<Value> {
        vec![
            Value::String(start_ms.to_string()),
            Value::String("100".into()),
            Value::String("102".into()),
            Value::String("99".into()),
            Value::String("101".into()),
            Value::String("12".into()),
            Value::String("1212".into()),
        ]
    }

    #[test]
    fn closed_rest_kline_has_exact_bounds_and_matching_typed_event() {
        let start = 1_700_000_000_000;
        let fetched = start + 60_000;
        let (candle, event) =
            parse_closed_rest_kline("BTCUSDT", "1", &row(start), fetched)
                .unwrap()
                .expect("closed bar");
        assert_eq!(candle.start_ms, start);
        assert_eq!(candle.end_ms, start + 59_999);
        assert!(candle.confirmed);
        assert!(matches!(
            event,
            NormalizedMarketEvent::Kline {
                ts_ms,
                start_ms,
                end_ms,
                confirmed: true,
                ..
            } if ts_ms == start + 59_999
                && start_ms == start
                && end_ms == start + 59_999
        ));
    }

    #[test]
    fn still_open_rest_kline_is_not_admitted_to_bootstrap() {
        let start = 1_700_000_000_000;
        let fetched = start + 59_999;
        assert!(parse_closed_rest_kline("BTCUSDT", "1", &row(start), fetched)
            .unwrap()
            .is_none());
    }
}
