use crate::{
    config::Config,
    replay::NormalizedMarketEvent,
    replay_pipeline::{run_production_replay, ReplayReport},
    runtime::RuntimeMarketConfig,
    simulator::{try_simulate_fill, FillRequest, LiquidityRole, SimulationConfig, SimulatedFill},
    types::Side,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Maximum age of an L50 touch used for replay fills. A replay evaluator must not
/// silently fill against an arbitrarily old book after a feed gap.
pub const DEFAULT_MAX_TOUCH_AGE_MS: u64 = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimulatedRoundTrip {
    pub signal_id: String,
    pub opened_at_ms: u64,
    pub closed_at_ms: u64,
    pub exit_reason: String,
    pub quantity: f64,
    pub entry_price: f64,
    pub exit_price: f64,
    pub fees_quote: f64,
    pub gross_pnl_quote: f64,
    pub net_pnl_quote: f64,
    pub entry_slippage_bps: f64,
    pub exit_slippage_bps: f64,
    pub total_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplaySimulationReport {
    pub replay_frames: usize,
    pub evaluation_ticks: usize,
    pub signal_events: usize,
    pub completed_round_trips: usize,
    pub skipped_no_book: usize,
    pub skipped_stale_book: usize,
    pub skipped_unfilled: usize,
    pub gross_pnl_quote: f64,
    pub fees_quote: f64,
    pub net_pnl_quote: f64,
    pub wins_after_costs: usize,
    pub losses_after_costs: usize,
    pub flat_after_costs: usize,
    pub win_rate_after_costs: Option<f64>,
    pub expectancy_quote: Option<f64>,
    pub round_trips: Vec<SimulatedRoundTrip>,
}

#[derive(Debug, Clone, Copy)]
struct Touch { ts_ms: u64, bid: f64, ask: f64, bid_qty: f64, ask_qty: f64 }

#[derive(Debug, Clone)]
struct OpenFill { side: Side, ts_ms: u64, fill: SimulatedFill }

/// Run the exact production replay path first, then cost the resulting immutable
/// signal lifecycle against the L50 touch known at each event timestamp. This is
/// offline/paper-only and never submits orders or mutates model weights.
pub fn run_replay_simulation(
    config: &Config,
    market: &RuntimeMarketConfig,
    events: Vec<NormalizedMarketEvent>,
    simulation: SimulationConfig,
) -> Result<ReplaySimulationReport> {
    ensure!(!events.is_empty(), "cannot simulate an empty recording");
    let replay_report = run_production_replay(config, market, events.clone(), None)?;
    simulate_replay_events(&events, &replay_report, simulation, DEFAULT_MAX_TOUCH_AGE_MS)
}

fn simulate_replay_events(
    market_events: &[NormalizedMarketEvent], replay: &ReplayReport,
    simulation: SimulationConfig, max_touch_age_ms: u64,
) -> Result<ReplaySimulationReport> {
    ensure!(max_touch_age_ms > 0, "max touch age must be positive");
    let touches = build_touch_timeline(market_events)?;
    let mut open: HashMap<String, OpenFill> = HashMap::new();
    let mut round_trips = Vec::new();
    let mut signal_events = 0usize;
    let mut skipped_no_book = 0usize;
    let mut skipped_stale_book = 0usize;
    let mut skipped_unfilled = 0usize;

    for event in &replay.emitted_events {
        let Some(signal) = event.signal.as_ref() else { continue; };
        if event.event_type == "signal" {
            signal_events += 1;
            if open.contains_key(&signal.id) { continue; }
            let Some(touch) = touch_at(&touches, event.ts_ms) else { skipped_no_book += 1; continue; };
            if event.ts_ms.saturating_sub(touch.ts_ms) > max_touch_age_ms { skipped_stale_book += 1; continue; }
            let side_sign = side_sign(&signal.side);
            let reference = (signal.entry_low + signal.entry_high) * 0.5;
            let visible = if side_sign > 0 { touch.ask_qty } else { touch.bid_qty };
            let fill = try_simulate_fill(simulation, FillRequest { side_sign, quantity: 1.0, reference_price: reference,
                best_bid: touch.bid, best_ask: touch.ask, visible_touch_quantity: visible, role: LiquidityRole::Taker,
                is_stop: false, gap_price: None })?;
            if fill.filled_quantity <= f64::EPSILON { skipped_unfilled += 1; continue; }
            open.insert(signal.id.clone(), OpenFill { side: signal.side.clone(), ts_ms: event.ts_ms, fill });
            continue;
        }

        if !is_terminal(&event.event_type) { continue; }
        let Some(entry) = open.get(&signal.id).cloned() else { continue; };
        let Some(touch) = touch_at(&touches, event.ts_ms) else { skipped_no_book += 1; continue; };
        if event.ts_ms.saturating_sub(touch.ts_ms) > max_touch_age_ms { skipped_stale_book += 1; continue; }
        // TP/SL normally carry the observed trade exit. Administrative lifecycle
        // closures (expiry/reversal/invalidation) are marked to the causal touch,
        // rather than being silently omitted from cost-adjusted evaluation.
        let observed_exit = signal.observed_exit_price.unwrap_or((touch.bid + touch.ask) * 0.5);
        let exit_side_sign = -side_sign(&entry.side);
        let visible = if exit_side_sign > 0 { touch.ask_qty } else { touch.bid_qty };
        let exit = try_simulate_fill(simulation, FillRequest { side_sign: exit_side_sign,
            quantity: entry.fill.filled_quantity, reference_price: observed_exit, best_bid: touch.bid, best_ask: touch.ask,
            visible_touch_quantity: visible, role: LiquidityRole::Taker, is_stop: event.event_type == "stop_loss",
            gap_price: if event.event_type == "stop_loss" { Some(observed_exit) } else { None } })?;
        let quantity = entry.fill.filled_quantity.min(exit.filled_quantity);
        if quantity <= f64::EPSILON { skipped_unfilled += 1; continue; }
        // A partial terminal fill is not a completed round trip. Keep the entry
        // open rather than silently discarding residual exposure/outcome state.
        if quantity + f64::EPSILON < entry.fill.filled_quantity { skipped_unfilled += 1; continue; }
        open.remove(&signal.id);
        let direction = side_sign(&entry.side) as f64;
        let gross = direction * (exit.fill_price - entry.fill.fill_price) * quantity;
        let entry_fee = entry.fill.fee_quote * quantity / entry.fill.filled_quantity;
        let exit_fee = exit.fee_quote * quantity / exit.filled_quantity;
        let fees = entry_fee + exit_fee;
        round_trips.push(SimulatedRoundTrip { signal_id: signal.id.clone(), opened_at_ms: entry.ts_ms,
            closed_at_ms: event.ts_ms, exit_reason: event.event_type.clone(), quantity,
            entry_price: entry.fill.fill_price, exit_price: exit.fill_price, fees_quote: fees,
            gross_pnl_quote: gross, net_pnl_quote: gross - fees, entry_slippage_bps: entry.fill.slippage_bps,
            exit_slippage_bps: exit.slippage_bps,
            total_latency_ms: entry.fill.total_latency_ms.saturating_add(exit.total_latency_ms) });
    }

    let gross_pnl_quote = round_trips.iter().map(|r| r.gross_pnl_quote).sum();
    let fees_quote = round_trips.iter().map(|r| r.fees_quote).sum();
    let net_pnl_quote = round_trips.iter().map(|r| r.net_pnl_quote).sum();
    let wins_after_costs = round_trips.iter().filter(|r| r.net_pnl_quote > 0.0).count();
    let losses_after_costs = round_trips.iter().filter(|r| r.net_pnl_quote < 0.0).count();
    let flat_after_costs = round_trips.len().saturating_sub(wins_after_costs + losses_after_costs);
    let win_rate_after_costs = (!round_trips.is_empty()).then(|| wins_after_costs as f64 / round_trips.len() as f64);
    let expectancy_quote = (!round_trips.is_empty()).then(|| net_pnl_quote / round_trips.len() as f64);

    Ok(ReplaySimulationReport { replay_frames: replay.frames, evaluation_ticks: replay.evaluation_ticks,
        signal_events, completed_round_trips: round_trips.len(), skipped_no_book, skipped_stale_book,
        skipped_unfilled, gross_pnl_quote, fees_quote, net_pnl_quote, wins_after_costs, losses_after_costs,
        flat_after_costs, win_rate_after_costs, expectancy_quote, round_trips })
}

fn side_sign(side: &Side) -> i8 { if matches!(side, Side::Long) { 1 } else { -1 } }
fn is_terminal(kind: &str) -> bool { matches!(kind, "tp2" | "stop_loss" | "expired" | "reversed" | "invalidated") }
fn touch_at(timeline: &BTreeMap<u64, Touch>, ts_ms: u64) -> Option<Touch> { timeline.range(..=ts_ms).next_back().map(|(_, t)| *t) }

fn build_touch_timeline(events: &[NormalizedMarketEvent]) -> Result<BTreeMap<u64, Touch>> {
    let mut bids: BTreeMap<ordered_float::OrderedFloat<f64>, f64> = BTreeMap::new();
    let mut asks: BTreeMap<ordered_float::OrderedFloat<f64>, f64> = BTreeMap::new();
    let mut timeline = BTreeMap::new();
    let mut sorted = events.to_vec(); sorted.sort_by_key(NormalizedMarketEvent::ts_ms);
    for event in sorted {
        let NormalizedMarketEvent::OrderBook { ts_ms, snapshot, bids: ub, asks: ua, .. } = event else { continue; };
        if snapshot { bids.clear(); asks.clear(); }
        apply_levels(&mut bids, ub); apply_levels(&mut asks, ua);
        let Some((bid, bid_qty)) = bids.iter().next_back() else { continue; };
        let Some((ask, ask_qty)) = asks.iter().next() else { continue; };
        ensure!(ask.0 >= bid.0, "replay book became crossed at {ts_ms}");
        timeline.insert(ts_ms, Touch { ts_ms, bid: bid.0, ask: ask.0, bid_qty: *bid_qty, ask_qty: *ask_qty });
    }
    Ok(timeline)
}
fn apply_levels(book: &mut BTreeMap<ordered_float::OrderedFloat<f64>, f64>, levels: Vec<(f64, f64)>) {
    for (price, qty) in levels { let key = ordered_float::OrderedFloat(price); if qty == 0.0 { book.remove(&key); } else { book.insert(key, qty); } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EngineEvent, SignalStatus, TradeSignal};
    fn signal(status: SignalStatus, exit: Option<f64>) -> TradeSignal { TradeSignal { id: "s1".into(), symbol: "BTCUSDT".into(), timeframe: "1".into(), side: Side::Long, status,
        created_at_ms: 110, entry_low: 100.0, entry_high: 100.0, stop_loss: 99.0, tp1: 101.0, tp2: 102.0, risk_reward_tp2: 2.0,
        confidence: 0.7, calibrated: false, invalidation: "below stop".into(), reasons: vec![], last_event_ms: 0, observed_exit_price: exit } }
    fn replay(events: Vec<EngineEvent>) -> ReplayReport { ReplayReport { frames: 1, evaluation_ticks: 1, emitted_events: events, final_features: None, final_analyses: vec![] } }
    fn book(ts_ms: u64) -> NormalizedMarketEvent { NormalizedMarketEvent::OrderBook { ts_ms, symbol: "BTCUSDT".into(), update_id: ts_ms, seq: ts_ms, snapshot: true, bids: vec![(99.9, 10.0)], asks: vec![(100.1, 10.0)] } }

    #[test]
    fn deterministic_round_trip_includes_costs_latency_and_metrics() {
        let market = vec![book(100), book(190)];
        let report = replay(vec![EngineEvent { ts_ms: 110, event_type: "signal".into(), alert: None, message: String::new(), signal: Some(signal(SignalStatus::Active, None)) },
            EngineEvent { ts_ms: 200, event_type: "tp2".into(), alert: None, message: String::new(), signal: Some(signal(SignalStatus::Tp2Hit, Some(102.0))) }]);
        let a = simulate_replay_events(&market, &report, SimulationConfig::default(), 2_000).unwrap();
        let b = simulate_replay_events(&market, &report, SimulationConfig::default(), 2_000).unwrap();
        assert_eq!(a, b); assert_eq!(a.completed_round_trips, 1); assert!(a.fees_quote > 0.0);
        assert_eq!(a.round_trips[0].total_latency_ms, 180); assert!(a.round_trips[0].net_pnl_quote < a.round_trips[0].gross_pnl_quote);
        let expected_win_rate = if a.net_pnl_quote > 0.0 { 1.0 } else { 0.0 };
        assert_eq!(a.win_rate_after_costs, Some(expected_win_rate));
        assert_eq!(a.expectancy_quote, Some(a.net_pnl_quote));
    }

    #[test]
    fn stale_book_fails_closed_for_entry() {
        let report = replay(vec![EngineEvent { ts_ms: 5_000, event_type: "signal".into(), alert: None, message: String::new(), signal: Some(signal(SignalStatus::Active, None)) }]);
        let out = simulate_replay_events(&[book(100)], &report, SimulationConfig::default(), 500).unwrap();
        assert_eq!(out.completed_round_trips, 0); assert_eq!(out.skipped_stale_book, 1);
    }

    #[test]
    fn administrative_terminal_marks_to_causal_touch() {
        let market = vec![book(100), book(190)];
        let report = replay(vec![EngineEvent { ts_ms: 110, event_type: "signal".into(), alert: None, message: String::new(), signal: Some(signal(SignalStatus::Active, None)) },
            EngineEvent { ts_ms: 200, event_type: "invalidated".into(), alert: None, message: String::new(), signal: Some(signal(SignalStatus::Invalidated, None)) }]);
        let out = simulate_replay_events(&market, &report, SimulationConfig::default(), 2_000).unwrap();
        assert_eq!(out.completed_round_trips, 1); assert_eq!(out.round_trips[0].exit_reason, "invalidated");
    }

    #[test]
    fn zero_size_delta_removes_touch_before_fill() {
        let market = vec![NormalizedMarketEvent::OrderBook { ts_ms: 100, symbol: "BTCUSDT".into(), update_id: 1, seq: 1, snapshot: true, bids: vec![(99.9, 10.0)], asks: vec![(100.1, 10.0), (100.2, 8.0)] },
            NormalizedMarketEvent::OrderBook { ts_ms: 105, symbol: "BTCUSDT".into(), update_id: 2, seq: 2, snapshot: false, bids: vec![], asks: vec![(100.1, 0.0)] }];
        let touches = build_touch_timeline(&market).unwrap(); assert_eq!(touch_at(&touches, 110).unwrap().ask, 100.2);
    }
}