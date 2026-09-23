use crate::{
    config::Config,
    market_event::apply_batch_to_internal,
    model::ModelEvaluator,
    replay::{DeterministicReplay, NormalizedMarketEvent},
    runtime::RuntimeMarketConfig,
    signal,
    state::InternalState,
    types::{EngineEvent, FeatureSnapshot, TimeframeAnalysis},
};
use anyhow::{ensure, Result};

#[derive(Debug, Clone)]
pub struct ReplayReport {
    pub frames: usize,
    pub evaluation_ticks: usize,
    pub emitted_events: Vec<EngineEvent>,
    pub final_features: Option<FeatureSnapshot>,
    pub final_analyses: Vec<TimeframeAnalysis>,
}

/// Drive a validated normalized recording through the exact production Rust
/// market-state mutation and decision core without live side effects.
///
/// Replay owns an isolated InternalState, so it cannot write the live journal,
/// mutate the paper account, send WebSocket messages, or ring alarms. An
/// optional verified ModelEvaluator lets research use the same champion model
/// path as live operation.
///
/// The deterministic clock is exchange-event time. Exact local receive latency
/// and the live 400ms scheduler phase are not claimed until receive timestamps
/// are persisted by the recorder.
pub fn run_production_replay(
    config: &Config,
    market: &RuntimeMarketConfig,
    events: Vec<NormalizedMarketEvent>,
    model: Option<ModelEvaluator>,
) -> Result<ReplayReport> {
    ensure!(!events.is_empty(), "cannot replay an empty recording");
    let replay = DeterministicReplay::try_new(events)?;

    let mut inner = InternalState::new();
    inner.model = model;
    inner.connected = true;
    inner.feed_stale = true;

    let mut emitted_events = Vec::new();
    let mut frames = 0_usize;
    let mut evaluation_ticks = 0_usize;

    for frame in replay {
        frames += 1;
        emitted_events.extend(apply_batch_to_internal(
            &mut inner,
            &market.symbol,
            std::slice::from_ref(&frame.event),
            frame.replay_ts_ms,
            config.stale_feed_ms,
        )?);

        // This is the same typed production decision core called by run_loop.
        // Bootstrap klines seed indicators, while no setup can pass before a
        // fresh L50 snapshot because feed_stale remains true.
        emitted_events.extend(signal::evaluate_internal(
            &mut inner,
            config,
            frame.replay_ts_ms,
            market,
        ));
        evaluation_ticks += 1;
    }

    Ok(ReplayReport {
        frames,
        evaluation_ticks,
        emitted_events,
        final_features: inner.features.clone(),
        final_analyses: inner.analyses.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn config() -> Config {
        Config {
            symbol: "BTCUSDT".into(),
            bybit_testnet: false,
            http_addr: "127.0.0.1:8787".parse().unwrap(),
            timeframe: "3".into(),
            min_signal_score: 0.74,
            min_rr: 1.8,
            signal_cooldown_secs: 90,
            max_spread_bps: 4.0,
            stale_feed_ms: 3_500,
            journal_path: PathBuf::from("unused-journal.jsonl"),
            market_record_path: PathBuf::from("unused-market.jsonl"),
            round_trip_cost_bps: 12.0,
            model_path: PathBuf::from("unused-model.json"),
        }
    }

    fn market() -> RuntimeMarketConfig {
        RuntimeMarketConfig {
            symbol: "BTCUSDT".into(),
            timeframe: "3".into(),
            generation: 1,
        }
    }

    fn bootstrap_events() -> Vec<NormalizedMarketEvent> {
        let anchor = 1_800_000_000_000_u64;
        let mut events = Vec::new();
        for interval in [1_u64, 3, 5, 15] {
            let duration = interval * 60_000;
            for i in 0..60_u64 {
                let start = anchor - (60 - i) * duration;
                let price = 100.0 + i as f64 * 0.1;
                events.push(NormalizedMarketEvent::Kline {
                    ts_ms: start + duration - 1,
                    symbol: "BTCUSDT".into(),
                    interval: interval.to_string(),
                    start_ms: start,
                    end_ms: start + duration - 1,
                    open: price,
                    high: price + 0.2,
                    low: price - 0.2,
                    close: price + 0.1,
                    volume: 10.0,
                    turnover: 10.0 * (price + 0.1),
                    confirmed: true,
                });
            }
        }
        events.push(NormalizedMarketEvent::OrderBook {
            ts_ms: anchor + 10,
            symbol: "BTCUSDT".into(),
            update_id: 1,
            seq: 100,
            snapshot: true,
            bids: vec![(105.8, 2.0)],
            asks: vec![(105.9, 2.0)],
        });
        events.push(NormalizedMarketEvent::Ticker {
            ts_ms: anchor + 20,
            symbol: "BTCUSDT".into(),
            last_price: Some(105.85),
            mark_price: Some(105.84),
            index_price: Some(105.83),
            open_interest: Some(10_000.0),
            funding_rate: Some(0.0001),
        });
        events
    }

    #[test]
    fn replay_runs_bootstrap_and_live_events_through_production_core_deterministically() {
        let events = bootstrap_events();
        let first = run_production_replay(&config(), &market(), events.clone(), None).unwrap();
        let second = run_production_replay(&config(), &market(), events, None).unwrap();

        assert_eq!(first.frames, 242);
        assert_eq!(first.evaluation_ticks, first.frames);
        assert_eq!(first.final_analyses.len(), 4);
        assert_eq!(second.final_analyses.len(), 4);

        let first_features = first.final_features.expect("features");
        let second_features = second.final_features.expect("features");
        assert_eq!(first_features.last_price, second_features.last_price);
        assert_eq!(first_features.spread_bps, second_features.spread_bps);
        assert_eq!(first_features.trend_15m_bps, second_features.trend_15m_bps);
        assert_eq!(first_features.book_imbalance, second_features.book_imbalance);
        assert!(first.emitted_events.iter().any(|event| event.event_type == "feed_recovered"));
        assert_eq!(
            first.emitted_events.iter().map(|event| event.event_type.as_str()).collect::<Vec<_>>(),
            second.emitted_events.iter().map(|event| event.event_type.as_str()).collect::<Vec<_>>(),
        );
    }
}
