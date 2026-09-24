use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

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
pub struct BinanceDepthCursor { final_update_id: u64, event_time_ms: u64 }
impl BinanceDepthCursor {
    pub fn final_update_id(self) -> u64 { self.final_update_id }
    pub fn event_time_ms(self) -> u64 { self.event_time_ms }
}

/// A causal, receive-time anchored exchange-clock estimate for research/context.
/// It never rewrites the source timestamps. The median offset is computed only
/// from samples observed up to the current message, so replay cannot learn a
/// future clock offset. Large clock jumps fail closed and require a fresh clock
/// epoch instead of silently reordering cross-venue evidence.
#[derive(Debug, Clone)]
pub struct VenueClockNormalizer {
    venue: String,
    offsets_ms: VecDeque<i64>,
    window: usize,
    max_abs_offset_ms: u64,
    max_offset_jump_ms: u64,
    last_exchange_ms: Option<u64>,
    last_normalized_ms: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedVenueTime {
    pub exchange_ms: u64,
    pub receive_ms: u64,
    pub normalized_ms: u64,
    pub estimated_offset_ms: i64,
    pub sample_count: usize,
}
impl VenueClockNormalizer {
    pub fn new(venue: impl Into<String>, window: usize, max_abs_offset_ms: u64, max_offset_jump_ms: u64) -> Result<Self> {
        let venue = venue.into();
        ensure!(!venue.trim().is_empty(), "venue clock name is required");
        ensure!(window > 0 && window <= 1024, "venue clock window must be in 1..=1024");
        ensure!(max_abs_offset_ms > 0 && max_offset_jump_ms > 0, "venue clock bounds must be positive");
        Ok(Self { venue, offsets_ms: VecDeque::with_capacity(window), window, max_abs_offset_ms, max_offset_jump_ms, last_exchange_ms: None, last_normalized_ms: None })
    }
    pub fn observe(&mut self, venue: &str, exchange_ms: u64, receive_ms: u64) -> Result<NormalizedVenueTime> {
        ensure!(venue == self.venue, "venue clock sample routed to wrong normalizer");
        ensure!(exchange_ms > 0 && receive_ms > 0, "venue clock timestamps are required");
        if let Some(last) = self.last_exchange_ms { ensure!(exchange_ms >= last, "venue exchange clock moved backwards"); }
        let raw = i128::from(receive_ms) - i128::from(exchange_ms);
        ensure!(raw.unsigned_abs() <= u128::from(self.max_abs_offset_ms), "venue clock offset exceeds configured bound");
        let raw = i64::try_from(raw).context("venue clock offset overflow")?;
        if let Some(previous) = median(&self.offsets_ms) {
            ensure!(raw.abs_diff(previous) <= self.max_offset_jump_ms, "venue clock offset jumped beyond configured bound");
        }
        self.offsets_ms.push_back(raw);
        if self.offsets_ms.len() > self.window { self.offsets_ms.pop_front(); }
        let estimate = median(&self.offsets_ms).context("venue clock estimate unavailable")?;
        let normalized = if estimate >= 0 { exchange_ms.checked_add(estimate as u64) } else { exchange_ms.checked_sub(estimate.unsigned_abs()) }
            .context("normalized venue timestamp overflow/underflow")?;
        if let Some(last) = self.last_normalized_ms { ensure!(normalized >= last, "normalized venue clock moved backwards"); }
        self.last_exchange_ms = Some(exchange_ms);
        self.last_normalized_ms = Some(normalized);
        Ok(NormalizedVenueTime { exchange_ms, receive_ms, normalized_ms: normalized, estimated_offset_ms: estimate, sample_count: self.offsets_ms.len() })
    }
}
fn median(samples: &VecDeque<i64>) -> Option<i64> {
    if samples.is_empty() { return None; }
    let mut values = samples.iter().copied().collect::<Vec<_>>();
    values.sort_unstable();
    Some(values[(values.len() - 1) / 2])
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
    let bids = levels(&v, "b")?;
    let asks = levels(&v, "a")?;
    ensure!(!bids.is_empty() || !asks.is_empty(), "Binance depth update has no levels");
    Ok(VenueBookUpdate { venue: "binance_usdm".into(), symbol: symbol.to_ascii_uppercase(), event_time_ms, transaction_time_ms, first_update_id, final_update_id, previous_final_update_id, bids, asks })
}

pub fn start_binance_after_snapshot(snapshot_last_update_id: u64, update: &VenueBookUpdate) -> Result<BinanceDepthCursor> {
    ensure!(snapshot_last_update_id > 0, "Binance snapshot lastUpdateId is required");
    validate_update(update)?;
    let next = snapshot_last_update_id.checked_add(1).context("Binance snapshot sequence overflow")?;
    ensure!(update.first_update_id <= next && next <= update.final_update_id, "first Binance delta does not bridge REST snapshot: expected sequence {} inside [{}, {}]", next, update.first_update_id, update.final_update_id);
    Ok(BinanceDepthCursor { final_update_id: update.final_update_id, event_time_ms: update.event_time_ms })
}
pub fn admit_binance_delta(previous: BinanceDepthCursor, update: &VenueBookUpdate) -> Result<BinanceDepthCursor> {
    validate_update(update)?;
    ensure!(update.event_time_ms >= previous.event_time_ms, "Binance event clock moved backwards");
    ensure!(update.final_update_id > previous.final_update_id, "stale/duplicate Binance depth update");
    ensure!(update.previous_final_update_id == previous.final_update_id, "Binance depth sequence gap: expected pu={}, got {}", previous.final_update_id, update.previous_final_update_id);
    ensure!(update.first_update_id <= previous.final_update_id.saturating_add(1), "Binance depth first update skips the expected next sequence");
    Ok(BinanceDepthCursor { final_update_id: update.final_update_id, event_time_ms: update.event_time_ms })
}
fn validate_update(update: &VenueBookUpdate) -> Result<()> {
    ensure!(update.venue == "binance_usdm", "wrong venue for Binance cursor");
    ensure!(update.first_update_id <= update.final_update_id, "invalid Binance update range");
    ensure!(update.event_time_ms > 0 && update.transaction_time_ms > 0, "Binance exchange timestamps are required");
    Ok(())
}
fn u64_field(v: &serde_json::Value, key: &str) -> Result<u64> { let n = v.get(key).and_then(|x| x.as_u64()).with_context(|| format!("Binance depth {key} missing"))?; ensure!(n > 0, "Binance depth {key} must be positive"); Ok(n) }
fn levels(v: &serde_json::Value, key: &str) -> Result<Vec<(f64, f64)>> {
    let rows = v.get(key).and_then(|x| x.as_array()).with_context(|| format!("Binance depth {key} missing"))?;
    rows.iter().map(|row| { let pair = row.as_array().context("Binance depth level must be [price, quantity]")?; ensure!(pair.len() == 2, "Binance depth level must contain exactly price and quantity"); let parse = |i: usize| -> Result<f64> { pair[i].as_str().context("Binance depth price/quantity must be strings")?.parse::<f64>().context("parse Binance depth number") }; let price = parse(0)?; let quantity = parse(1)?; ensure!(price.is_finite() && price > 0.0, "Binance depth price must be finite and positive"); ensure!(quantity.is_finite() && quantity >= 0.0, "Binance depth quantity must be finite and non-negative"); Ok((price, quantity)) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(e: u64, u_first: u64, u_final: u64, pu: u64) -> String { format!(r#"{{"e":"depthUpdate","E":{e},"T":{e},"s":"BTCUSDT","U":{u_first},"u":{u_final},"pu":{pu},"b":[["65000.1","1.25"],["64999.9","0"]],"a":[["65000.2","2.5"]]}}"#) }
    #[test] fn parses_recorded_shape_and_preserves_zero_size_deletion() { let u=parse_binance_usdm_depth(&event(1_700_000_000_000,101,102,100)).unwrap(); assert_eq!(u.venue,"binance_usdm"); assert_eq!(u.symbol,"BTCUSDT"); assert_eq!(u.previous_final_update_id,100); assert_eq!(u.bids[1],(64999.9,0.0)); }
    #[test] fn snapshot_anchor_and_continuity_fail_closed_on_partial_or_gapped_book() { let first=parse_binance_usdm_depth(&event(1000,101,102,100)).unwrap(); assert!(start_binance_after_snapshot(99,&first).is_err()); let c1=start_binance_after_snapshot(100,&first).unwrap(); let second=parse_binance_usdm_depth(&event(1001,103,104,102)).unwrap(); let c2=admit_binance_delta(c1,&second).unwrap(); assert_eq!(c2.final_update_id(),104); assert_eq!(c2.event_time_ms(),1001); assert!(admit_binance_delta(c2,&second).is_err()); let gap=parse_binance_usdm_depth(&event(1002,106,107,105)).unwrap(); assert!(admit_binance_delta(c2,&gap).is_err()); let backwards=parse_binance_usdm_depth(&event(999,105,106,104)).unwrap(); assert!(admit_binance_delta(c2,&backwards).is_err()); }
    #[test] fn clock_normalization_is_causal_bounded_and_monotonic() { let mut c=VenueClockNormalizer::new("binance_usdm",3,5_000,250).unwrap(); let a=c.observe("binance_usdm",1000,1100).unwrap(); assert_eq!(a.normalized_ms,1100); let b=c.observe("binance_usdm",1100,1210).unwrap(); assert_eq!(b.estimated_offset_ms,100); let d=c.observe("binance_usdm",1200,1320).unwrap(); assert_eq!(d.estimated_offset_ms,110); assert_eq!(d.sample_count,3); assert!(d.normalized_ms>=b.normalized_ms); }
    #[test] fn clock_normalization_rejects_wrong_venue_reversal_jump_and_extreme_skew() { let mut c=VenueClockNormalizer::new("binance_usdm",5,1000,100).unwrap(); c.observe("binance_usdm",1000,1100).unwrap(); assert!(c.observe("okx",1100,1200).is_err()); assert!(c.observe("binance_usdm",999,1099).is_err()); assert!(c.observe("binance_usdm",1100,1400).is_err()); let mut extreme=VenueClockNormalizer::new("binance_usdm",3,50,20).unwrap(); assert!(extreme.observe("binance_usdm",1000,1100).is_err()); }
    #[test] fn future_clock_samples_cannot_change_past_normalized_time() { let mut prefix=VenueClockNormalizer::new("binance_usdm",5,5000,500).unwrap(); let first=prefix.observe("binance_usdm",10_000,10_100).unwrap(); let mut extended=VenueClockNormalizer::new("binance_usdm",5,5000,500).unwrap(); let same=extended.observe("binance_usdm",10_000,10_100).unwrap(); extended.observe("binance_usdm",10_100,10_350).unwrap(); extended.observe("binance_usdm",10_200,10_450).unwrap(); assert_eq!(first,same); }
    #[test] fn malformed_or_unsafe_events_fail_closed() { assert!(parse_binance_usdm_depth("not-json").is_err()); assert!(parse_binance_usdm_depth(r#"{"e":"trade"}"#).is_err()); assert!(parse_binance_usdm_depth(&event(1000,103,102,101)).is_err()); let negative=event(1000,101,102,100).replace("\"1.25\"","\"-1\""); assert!(parse_binance_usdm_depth(&negative).is_err()); let nan=event(1000,101,102,100).replace("\"1.25\"","\"NaN\""); assert!(parse_binance_usdm_depth(&nan).is_err()); }
}
