use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// Normalized L2 update from a secondary venue. This is context-only: it is
/// deliberately not accepted by the Bybit execution/signal book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VenueBookUpdate {
    pub venue: String,
    pub symbol: String,
    pub event_time_ms: u64,
    pub transaction_time_ms: u64,
    pub first_update_id: u64,
    pub final_update_id: u64,
    pub previous_final_update_id: u64,
    pub bids: Vec<(f64, f64)>,
    pub asks: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinanceDepthCursor {
    final_update_id: u64,
    event_time_ms: u64,
}

impl BinanceDepthCursor {
    pub fn final_update_id(self) -> u64 { self.final_update_id }
    pub fn event_time_ms(self) -> u64 { self.event_time_ms }
}

/// Parse a Binance USD-M `@depth@100ms` diff event without losing sequence or
/// exchange-clock provenance. Zero quantities are retained because they are
/// deletion instructions, not empty liquidity observations.
pub fn parse_binance_usdm_depth(raw: &str) -> Result<VenueBookUpdate> {
    let v: serde_json::Value = serde_json::from_str(raw).context("decode Binance depth event")?;
    ensure!(v.get("e").and_then(|x| x.as_str()) == Some("depthUpdate"), "expected Binance depthUpdate event");
    let symbol = v.get("s").and_then(|x| x.as_str()).context("Binance depth symbol missing")?.trim();
    ensure!(!symbol.is_empty(), "Binance depth symbol is empty");
    let event_time_ms = u64_field(&v, "E")?;
    let transaction_time_ms = u64_field(&v, "T")?;
    let first_update_id = u64_field(&v, "U")?;
    let final_update_id = u64_field(&v, "u")?;
    let previous_final_update_id = u64_field(&v, "pu")?;
    ensure!(first_update_id <= final_update_id, "Binance depth update range is reversed");
    ensure!(event_time_ms > 0 && transaction_time_ms > 0, "Binance exchange timestamps are required");
    // Exchange transaction time may lead/lag event time slightly; both clocks
    // are preserved and later normalization must explicitly choose one.
    let bids = levels(&v, "b")?;
    let asks = levels(&v, "a")?;
    ensure!(!bids.is_empty() || !asks.is_empty(), "Binance depth update has no levels");
    Ok(VenueBookUpdate {
        venue: "binance_usdm".into(), symbol: symbol.to_ascii_uppercase(), event_time_ms,
        transaction_time_ms, first_update_id, final_update_id, previous_final_update_id, bids, asks,
    })
}

/// Validate source-order continuity. Duplicate/stale updates and gaps fail
/// closed instead of being folded into cross-venue context. A caller must
/// reacquire a REST snapshot after an error before accepting more deltas.
pub fn admit_binance_delta(cursor: Option<BinanceDepthCursor>, update: &VenueBookUpdate) -> Result<BinanceDepthCursor> {
    ensure!(update.venue == "binance_usdm", "wrong venue for Binance cursor");
    ensure!(update.first_update_id <= update.final_update_id, "invalid Binance update range");
    if let Some(previous) = cursor {
        ensure!(update.event_time_ms >= previous.event_time_ms, "Binance event clock moved backwards");
        ensure!(update.final_update_id > previous.final_update_id, "stale/duplicate Binance depth update");
        ensure!(update.previous_final_update_id == previous.final_update_id,
            "Binance depth sequence gap: expected pu={}, got {}", previous.final_update_id, update.previous_final_update_id);
        ensure!(update.first_update_id <= previous.final_update_id.saturating_add(1),
            "Binance depth first update skips the expected next sequence");
    }
    Ok(BinanceDepthCursor { final_update_id: update.final_update_id, event_time_ms: update.event_time_ms })
}

fn u64_field(v: &serde_json::Value, key: &str) -> Result<u64> {
    let n = v.get(key).and_then(|x| x.as_u64()).with_context(|| format!("Binance depth {key} missing"))?;
    ensure!(n > 0, "Binance depth {key} must be positive");
    Ok(n)
}

fn levels(v: &serde_json::Value, key: &str) -> Result<Vec<(f64, f64)>> {
    let rows = v.get(key).and_then(|x| x.as_array()).with_context(|| format!("Binance depth {key} missing"))?;
    rows.iter().map(|row| {
        let pair = row.as_array().context("Binance depth level must be [price, quantity]")?;
        ensure!(pair.len() == 2, "Binance depth level must contain exactly price and quantity");
        let parse = |i: usize| -> Result<f64> {
            let s = pair[i].as_str().context("Binance depth price/quantity must be strings")?;
            s.parse::<f64>().context("parse Binance depth number")
        };
        let price = parse(0)?;
        let quantity = parse(1)?;
        ensure!(price.is_finite() && price > 0.0, "Binance depth price must be finite and positive");
        ensure!(quantity.is_finite() && quantity >= 0.0, "Binance depth quantity must be finite and non-negative");
        Ok((price, quantity))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(e: u64, u_first: u64, u_final: u64, pu: u64) -> String {
        format!(r#"{{"e":"depthUpdate","E":{e},"T":{e},"s":"BTCUSDT","U":{u_first},"u":{u_final},"pu":{pu},"b":[["65000.1","1.25"],["64999.9","0"]],"a":[["65000.2","2.5"]]}}"#)
    }

    #[test]
    fn parses_recorded_shape_and_preserves_zero_size_deletion() {
        let u = parse_binance_usdm_depth(&event(1_700_000_000_000, 101, 102, 100)).unwrap();
        assert_eq!(u.venue, "binance_usdm");
        assert_eq!(u.symbol, "BTCUSDT");
        assert_eq!(u.first_update_id, 101);
        assert_eq!(u.final_update_id, 102);
        assert_eq!(u.previous_final_update_id, 100);
        assert_eq!(u.bids[1], (64999.9, 0.0));
    }

    #[test]
    fn continuity_accepts_chain_and_rejects_stale_gap_and_clock_reversal() {
        let first = parse_binance_usdm_depth(&event(1000, 101, 102, 100)).unwrap();
        let c1 = admit_binance_delta(None, &first).unwrap();
        let second = parse_binance_usdm_depth(&event(1001, 103, 104, 102)).unwrap();
        let c2 = admit_binance_delta(Some(c1), &second).unwrap();
        assert_eq!(c2.final_update_id(), 104);
        assert_eq!(c2.event_time_ms(), 1001);
        assert!(admit_binance_delta(Some(c2), &second).is_err());
        let gap = parse_binance_usdm_depth(&event(1002, 106, 107, 105)).unwrap();
        assert!(admit_binance_delta(Some(c2), &gap).is_err());
        let backwards = parse_binance_usdm_depth(&event(999, 105, 106, 104)).unwrap();
        assert!(admit_binance_delta(Some(c2), &backwards).is_err());
    }

    #[test]
    fn malformed_or_unsafe_events_fail_closed() {
        assert!(parse_binance_usdm_depth("not-json").is_err());
        assert!(parse_binance_usdm_depth(r#"{"e":"trade"}"#).is_err());
        let bad = event(1000, 103, 102, 101);
        assert!(parse_binance_usdm_depth(&bad).is_err());
        let negative = event(1000, 101, 102, 100).replace("\"1.25\"", "\"-1\"");
        assert!(parse_binance_usdm_depth(&negative).is_err());
        let nan = event(1000, 101, 102, 100).replace("\"1.25\"", "\"NaN\"");
        assert!(parse_binance_usdm_depth(&nan).is_err());
    }
}
