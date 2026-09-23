use crate::replay::NormalizedMarketEvent;
use serde_json::Value;

/// Convert a Bybit V5 public websocket payload into the exact typed events
/// persisted by the market recorder. Control/heartbeat messages yield no events.
pub fn normalize_message(text: &str) -> Vec<NormalizedMarketEvent> {
    let Ok(root) = serde_json::from_str::<Value>(text) else { return Vec::new(); };
    let Some(topic) = root.get("topic").and_then(Value::as_str) else { return Vec::new(); };
    let symbol = topic.rsplit('.').next().unwrap_or_default().to_string();
    let root_ts = root.get("ts").and_then(parse_u64).unwrap_or_default();
    let mut out = Vec::new();

    if topic.starts_with("kline.") {
        if let Some(data) = root.get("data").and_then(Value::as_array) {
            for item in data {
                out.push(NormalizedMarketEvent::Kline {
                    ts_ms: item.get("timestamp").and_then(parse_u64).unwrap_or(root_ts),
                    symbol: symbol.clone(),
                    interval: item.get("interval").and_then(Value::as_str).unwrap_or("1").to_string(),
                    start_ms: item.get("start").and_then(parse_u64).unwrap_or_default(),
                    end_ms: item.get("end").and_then(parse_u64).unwrap_or_default(),
                    open: item.get("open").and_then(parse_f64).unwrap_or_default(),
                    high: item.get("high").and_then(parse_f64).unwrap_or_default(),
                    low: item.get("low").and_then(parse_f64).unwrap_or_default(),
                    close: item.get("close").and_then(parse_f64).unwrap_or_default(),
                    volume: item.get("volume").and_then(parse_f64).unwrap_or_default(),
                    turnover: item.get("turnover").and_then(parse_f64).unwrap_or_default(),
                    confirmed: item.get("confirm").and_then(Value::as_bool).unwrap_or(false),
                });
            }
        }
    } else if topic.starts_with("publicTrade.") {
        if let Some(data) = root.get("data").and_then(Value::as_array) {
            for trade in data {
                out.push(NormalizedMarketEvent::Trade {
                    ts_ms: trade.get("T").and_then(parse_u64).unwrap_or(root_ts),
                    symbol: symbol.clone(),
                    price: trade.get("p").and_then(parse_f64).unwrap_or_default(),
                    qty: trade.get("v").and_then(parse_f64).unwrap_or_default(),
                    side: trade.get("S").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase(),
                });
            }
        }
    } else if topic.starts_with("orderbook.") {
        if let Some(data) = root.get("data") {
            let update_id = data.get("u").and_then(parse_u64).unwrap_or_default();
            out.push(NormalizedMarketEvent::OrderBook {
                ts_ms: data.get("cts").and_then(parse_u64).unwrap_or(root_ts),
                symbol,
                update_id,
                seq: data.get("seq").and_then(parse_u64).unwrap_or_default(),
                snapshot: root.get("type").and_then(Value::as_str) == Some("snapshot") || update_id == 1,
                bids: levels(data.get("b")),
                asks: levels(data.get("a")),
            });
        }
    } else if topic.starts_with("tickers.") {
        if let Some(data) = root.get("data") {
            let data = if data.is_array() { data.get(0).unwrap_or(data) } else { data };
            out.push(NormalizedMarketEvent::Ticker {
                ts_ms: root_ts,
                symbol,
                last_price: data.get("lastPrice").and_then(parse_f64),
                mark_price: data.get("markPrice").and_then(parse_f64),
                index_price: data.get("indexPrice").and_then(parse_f64),
                open_interest: data.get("openInterest").and_then(parse_f64),
                funding_rate: data.get("fundingRate").and_then(parse_f64),
            });
        }
    } else if topic.starts_with("allLiquidation.") {
        if let Some(data) = root.get("data").and_then(Value::as_array) {
            for liq in data {
                out.push(NormalizedMarketEvent::Liquidation {
                    ts_ms: liq.get("T").and_then(parse_u64).unwrap_or(root_ts),
                    symbol: symbol.clone(),
                    price: liq.get("p").and_then(parse_f64).unwrap_or_default(),
                    qty: liq.get("v").and_then(parse_f64).unwrap_or_default(),
                    side: liq.get("S").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase(),
                });
            }
        }
    }
    out
}

fn levels(value: Option<&Value>) -> Vec<(f64, f64)> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(|level| {
        let row = level.as_array()?;
        Some((parse_f64(row.first()?)?, parse_f64(row.get(1)?)?))
    }).collect()
}

fn parse_f64(value: &Value) -> Option<f64> {
    value.as_str().and_then(|x| x.parse().ok()).or_else(|| value.as_f64())
}
fn parse_u64(value: &Value) -> Option<u64> {
    value.as_str().and_then(|x| x.parse().ok()).or_else(|| value.as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_exact_book_delta_without_losing_zero_size_deletes() {
        let events = normalize_message(r#"{"topic":"orderbook.50.BTCUSDT","type":"delta","ts":1001,"data":{"u":42,"seq":77,"cts":999,"b":[["100","0"]],"a":[["101","2.5"]]}}"#);
        assert_eq!(events.len(), 1);
        let NormalizedMarketEvent::OrderBook { ts_ms, symbol, update_id, seq, snapshot, bids, asks } = &events[0] else { panic!("wrong event") };
        assert_eq!((*ts_ms, symbol.as_str(), *update_id, *seq, *snapshot), (999, "BTCUSDT", 42, 77, false));
        assert_eq!(bids, &vec![(100.0, 0.0)]);
        assert_eq!(asks, &vec![(101.0, 2.5)]);
    }

    #[test]
    fn normalizes_kline_with_exact_candle_bounds() {
        let events = normalize_message(r#"{\"topic\":\"kline.1.BTCUSDT\",\"ts\":1700000060000,\"data\":[{\"start\":1700000000000,\"end\":1700000059999,\"timestamp\":1700000050000,\"interval\":\"1\",\"open\":\"100\",\"high\":\"102\",\"low\":\"99\",\"close\":\"101\",\"volume\":\"12\",\"turnover\":\"1212\",\"confirm\":true}]}"#);
        assert_eq!(events.len(), 1);
        let NormalizedMarketEvent::Kline { ts_ms, start_ms, end_ms, interval, confirmed, .. } = &events[0] else { panic!("wrong event") };
        assert_eq!((*ts_ms, *start_ms, *end_ms, interval.as_str(), *confirmed), (1_700_000_050_000, 1_700_000_000_000, 1_700_000_059_999, "1", true));
    }

    #[test]
    fn normalizes_trade_batch_with_exchange_timestamps() {
        let events = normalize_message(r#"{"topic":"publicTrade.ETHUSDT","ts":10,"data":[{"T":11,"p":"2500.5","v":"0.2","S":"Buy"},{"T":12,"p":"2500","v":"0.1","S":"Sell"}]}"#);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], NormalizedMarketEvent::Trade { ts_ms: 11, symbol, side, .. } if symbol == "ETHUSDT" && side == "buy"));
        assert!(matches!(&events[1], NormalizedMarketEvent::Trade { ts_ms: 12, side, .. } if side == "sell"));
    }

    #[test]
    fn ignores_control_messages() {
        assert!(normalize_message(r#"{"op":"pong"}"#).is_empty());
    }
}
