use crate::{
    market_event::apply_batch_to_internal,
    replay::{DeterministicReplay, NormalizedMarketEvent},
    runtime::RuntimeMarketConfig,
    signal,
    state::AppState,
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

/// Drive a validated normalized recording through the production Rust
/// market-state mutation and signal-evaluation functions.
///
/// This is deliberately side-effect free with respect to the live journal,
/// WebSocket clients, browser/terminal alarms and paper account: replay events
/// are returned to the caller rather than published. The supplied AppState is
/// mutated into replay state, so callers should use an isolated research state.
///
/// The deterministic clock is exchange-event time. This is suitable for
/// reproducible research; exact wall-clock arrival/400ms scheduler latency is
/// not claimed until receive timestamps are persisted as part of the recorder.
pub fn run_production_replay(
    state: &AppState,
    market: &RuntimeMarketConfig,
    events: Vec<NormalizedMarketEvent>,
) -> Result<ReplayReport> {
    ensure!(!events.is_empty(), "cannot replay an empty recording");
    let replay = DeterministicReplay::try_new(events)?;

    {
        let mut inner = state.inner.write();
        inner.reset_market();
        inner.connected = true;
        inner.feed_stale = true;
    }

    let mut emitted_events = Vec::new();
    let mut frames = 0_usize;
    let mut evaluation_ticks = 0_usize;

    for frame in replay {
        frames += 1;
        let lifecycle_events = {
            let mut inner = state.inner.write();
            apply_batch_to_internal(
                &mut inner,
                &market.symbol,
                std::slice::from_ref(&frame.event),
                frame.replay_ts_ms,
                state.config.stale_feed_ms,
            )?
        };
        emitted_events.extend(lifecycle_events);

        // Use the exact production evaluator. Historical REST bootstrap klines
        // cannot admit setups before a fresh L50 snapshot because feed_stale
        // remains true, so they seed indicators without inventing entries.
        emitted_events.extend(signal::evaluate_once(
            state,
            frame.replay_ts_ms,
            market,
        ));
        evaluation_ticks += 1;
    }

    let inner = state.inner.read();
    Ok(ReplayReport {
        frames,
        evaluation_ticks,
        emitted_events,
        final_features: inner.features.clone(),
        final_analyses: inner.analyses.clone(),
    })
}
