use crate::{
    config::Config,
    journal::Journal,
    runtime,
    types::{Candle, ChartFlag, EngineEvent, EngineSnapshot, FeatureSnapshot, SignalStatus, TradeSignal, TimeframeAnalysis},
};
use parking_lot::RwLock;
use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    sync::Arc,
};
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub(crate) struct FlowSample {
    pub ts_ms: u64,
    pub signed_qty: f64,
    pub signed_notional: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct OiSample {
    pub ts_ms: u64,
    pub value: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingSignalConfirmation {
    pub candle_ms: u64,
    pub side: String,
    pub setup: String,
    pub first_pass_ms: u64,
    pub pass_ticks: u32,
}

#[derive(Debug)]
pub(crate) struct InternalState {
    pub connected: bool,
    pub feed_stale: bool,
    pub last_market_event_ms: u64,
    pub last_price: Option<f64>,
    pub mark_price: Option<f64>,
    pub index_price: Option<f64>,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub book_imbalance: f64,
    pub book_imbalance_top5: f64,
    pub microprice_bps: f64,
    pub bid_depth_slope: f64,
    pub ask_depth_slope: f64,
    pub depth_pressure: f64,
    pub previous_bid_depth: f64,
    pub previous_ask_depth: f64,
    pub orderbook_bids: HashMap<String, (f64, f64)>,
    pub orderbook_asks: HashMap<String, (f64, f64)>,
    pub orderbook_update_id: u64,
    pub orderbook_seq: u64,
    pub orderbook_event_ms: u64,
    pub open_interest: Option<f64>,
    pub previous_open_interest: Option<f64>,
    pub funding_rate: Option<f64>,
    pub bars: HashMap<String, VecDeque<Candle>>,
    pub trade_flow: VecDeque<FlowSample>,
    pub liquidation_flow: VecDeque<FlowSample>,
    pub oi_samples: VecDeque<OiSample>,
    pub features: Option<FeatureSnapshot>,
    pub active_signal: Option<TradeSignal>,
    pub signals: HashMap<String, TradeSignal>,
    pub signal_history: VecDeque<TradeSignal>,
    pub analyses: Vec<TimeframeAnalysis>,
    pub admitted_candles: HashMap<String, u64>,
    pub pending_signal_confirmations: HashMap<String, PendingSignalConfirmation>,
    pub last_signal_at_ms: u64,
    pub jev_responded: bool,
    pub model: Option<crate::model::ModelEvaluator>,
    pub updated_at_ms: u64,
}

impl InternalState {
    pub(crate) fn new() -> Self {
        let now = now_ms();
        Self {
            connected: false,
            feed_stale: false,
            last_market_event_ms: now,
            last_price: None,
            mark_price: None,
            index_price: None,
            bid: None,
            ask: None,
            book_imbalance: 0.0,
            book_imbalance_top5: 0.0,
            microprice_bps: 0.0,
            bid_depth_slope: 0.0,
            ask_depth_slope: 0.0,
            depth_pressure: 0.0,
            previous_bid_depth: 0.0,
            previous_ask_depth: 0.0,
            orderbook_bids: HashMap::new(),
            orderbook_asks: HashMap::new(),
            orderbook_update_id: 0,
            orderbook_seq: 0,
            orderbook_event_ms: 0,
            open_interest: None,
            previous_open_interest: None,
            funding_rate: None,
            bars: HashMap::new(),
            trade_flow: VecDeque::new(),
            liquidation_flow: VecDeque::new(),
            oi_samples: VecDeque::new(),
            features: None,
            active_signal: None,
            signals: HashMap::new(),
            signal_history: VecDeque::new(),
            analyses: Vec::new(),
            admitted_candles: HashMap::new(),
            pending_signal_confirmations: HashMap::new(),
            last_signal_at_ms: 0,
            jev_responded: false,
            model: None,
            updated_at_ms: now,
        }
    }

    pub fn upsert_candle(&mut self, candle: Candle) {
        let deque = self.bars.entry(candle.interval.clone()).or_default();
        let position = deque.iter().position(|existing| existing.start_ms >= candle.start_ms);
        if let Some(index) = position.filter(|&index| deque[index].start_ms == candle.start_ms) {
            deque[index] = candle;
        } else {
            deque.insert(position.unwrap_or(deque.len()), candle);
            while deque.len() > 1500 {
                deque.pop_front();
            }
        }
    }

    pub fn push_trade_flow(&mut self, sample: FlowSample) {
        self.trade_flow.push_back(sample);
        while self.trade_flow.len() > 4000 {
            self.trade_flow.pop_front();
        }
    }

    pub fn push_liquidation_flow(&mut self, sample: FlowSample) {
        self.liquidation_flow.push_back(sample);
        while self.liquidation_flow.len() > 2000 {
            self.liquidation_flow.pop_front();
        }
    }

    pub fn push_oi_sample(&mut self, sample: OiSample) {
        if self
            .oi_samples
            .back()
            .is_some_and(|last| last.value == sample.value)
        {
            return;
        }
        self.oi_samples.push_back(sample);
        while self.oi_samples.len() > 10_000 {
            self.oi_samples.pop_front();
        }
    }

    pub fn archive_signal(&mut self, signal: TradeSignal) {
        if self.signal_history.back().is_some_and(|last| {
            last.id == signal.id && last.status == signal.status && last.last_event_ms == signal.last_event_ms
        }) {
            return;
        }
        self.signal_history.push_back(signal);
        while self.signal_history.len() > 500 {
            self.signal_history.pop_front();
        }
    }

    pub fn invalidate_market_signals(&mut self, now: u64, reason: &str) -> Vec<TradeSignal> {
        let drained: Vec<_> = self.signals.drain().map(|(_, signal)| signal).collect();
        let mut invalidated = Vec::new();
        for mut signal in drained {
            if matches!(signal.status, SignalStatus::Active | SignalStatus::Tp1Hit) {
                signal.status = SignalStatus::Invalidated;
                signal.last_event_ms = now;
                signal.observed_exit_price = self.last_price;
                signal.invalidation = format!("{reason} Previous setup retained in signal history.");
                invalidated.push(signal.clone());
            }
            self.archive_signal(signal);
        }
        self.active_signal = None;
        invalidated
    }

    pub fn reset_market(&mut self) {
        let now = now_ms();
        self.connected = false;
        self.feed_stale = true;
        self.last_market_event_ms = now;
        self.last_price = None;
        self.mark_price = None;
        self.index_price = None;
        self.bid = None;
        self.ask = None;
        self.book_imbalance = 0.0;
        self.book_imbalance_top5 = 0.0;
        self.microprice_bps = 0.0;
        self.bid_depth_slope = 0.0;
        self.ask_depth_slope = 0.0;
        self.depth_pressure = 0.0;
        self.previous_bid_depth = 0.0;
        self.previous_ask_depth = 0.0;
        self.orderbook_bids.clear();
        self.orderbook_asks.clear();
        self.orderbook_update_id = 0;
        self.orderbook_seq = 0;
        self.orderbook_event_ms = 0;
        self.open_interest = None;
        self.previous_open_interest = None;
        self.funding_rate = None;
        self.bars.clear();
        self.signals.clear();
        self.analyses.clear();
        self.admitted_candles.clear();
        self.pending_signal_confirmations.clear();
        self.trade_flow.clear();
        self.liquidation_flow.clear();
        self.oi_samples.clear();
        self.features = None;
        self.active_signal = None;
        self.last_signal_at_ms = 0;
        self.updated_at_ms = now;
    }

    pub fn reset_signal_context(&mut self) {
        self.features = None;
        self.active_signal = None;
        self.pending_signal_confirmations.clear();
        self.last_signal_at_ms = 0;
        self.updated_at_ms = now_ms();
    }

    pub fn prune_flows(&mut self, now: u64) {
        let min_trade_ts = now.saturating_sub(20_000);
        let min_liq_ts = now.saturating_sub(60_000);
        let min_oi_ts = now.saturating_sub(600_000);
        while self.trade_flow.front().is_some_and(|x| x.ts_ms < min_trade_ts) {
            self.trade_flow.pop_front();
        }
        while self.liquidation_flow.front().is_some_and(|x| x.ts_ms < min_liq_ts) {
            self.liquidation_flow.pop_front();
        }
        while self.oi_samples.front().is_some_and(|x| x.ts_ms < min_oi_ts) {
            self.oi_samples.pop_front();
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub paper: Arc<parking_lot::Mutex<crate::paper::Paper>>,
    pub(crate) inner: Arc<RwLock<InternalState>>,
    pub events: broadcast::Sender<EngineEvent>,
    journal: Arc<Journal>,
    chart_flags: Arc<RwLock<VecDeque<ChartFlag>>>,
}

impl AppState {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let (events, _) = broadcast::channel(512);
        let model = match crate::model::ModelEvaluator::load_from_file(&config.model_path) {
            Ok(m) if m.deployment_allowed() => Some(m),
            Ok(_) => {tracing::warn!("Model blocked: independent live outcome validation required");None},
            Err(e) => {
                tracing::info!(path = %config.model_path.display(), error = %e, "No calibrated champion model loaded; falling back to rule-based paper scoring");
                None
            }
        };
        let mut inner_state = InternalState::new();
        inner_state.model = model;
        Ok(Self {
            paper: Arc::new(parking_lot::Mutex::new(crate::paper::Paper::open(std::env::var("A6_PAPER_PATH").unwrap_or_else(|_|"data/paper-account.json".into()).into())?)),
            chart_flags: Arc::new(RwLock::new(crate::flags::load(&config.journal_path))),
            journal: Arc::new(Journal::open(&config.journal_path)?),
            config,
            inner: Arc::new(RwLock::new(inner_state)),
            events,
        })
    }

    pub fn publish(&self, event: EngineEvent) {
        if let Some(flag) = crate::flags::from_event(&event) {
            crate::flags::append(&mut self.chart_flags.write(), flag);
        }
        if event.event_type == "jev_review" {
            self.inner.write().jev_responded = true;
        }
        self.journal.append(&event);
        if event.alert.is_some() {
            print!("\x07");
            let _ = std::io::stdout().flush();
        }
        let _ = self.events.send(event);
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let s = self.inner.read();
        let bars = |tf: &str| {
            s.bars
                .get(tf)
                .map(|x| x.iter().cloned().collect())
                .unwrap_or_default()
        };
        let now = now_ms();
        let market = runtime::current();
        let pv = self.paper.lock().view();
        EngineSnapshot {
            symbol: market.symbol.clone(),
            timeframe: market.timeframe.clone(),
            market_generation: market.generation,
            connected: s.connected,
            feed_stale: s.feed_stale,
            feed_age_ms: now.saturating_sub(s.last_market_event_ms),
            last_price: s.last_price,
            mark_price: s.mark_price,
            index_price: s.index_price,
            features: s.features.clone(),
            active_signal: s.signals.get(&market.timeframe).cloned(),
            timeframe_signals: crate::indicators::TIMEFRAMES.iter().filter_map(|tf| s.signals.get(*tf).cloned()).collect(),
            signal_history: s.signal_history.iter().filter(|signal| signal.symbol == market.symbol).cloned().collect(),
            analyses: s.analyses.clone(),
            chart_flags: self.chart_flags.read().iter().filter(|f| f.symbol == market.symbol).cloned().collect(),
            candles_1m: bars("1"),
            candles_3m: bars("3"),
            candles_5m: bars("5"),
            candles_15m: bars("15"),
            paper_balance: pv.account.balance,
            paper_equity: pv.equity,
            paper_unrealized: pv.unrealized,
            paper_realized: pv.realized,
            paper_win_rate: pv.win_rate,
            paper_profit_factor: pv.profit_factor,
            paper_max_drawdown: pv.max_closed_drawdown,
            paper_fees: pv.fees,
            paper_open_count: pv.account.positions.iter().filter(|p| p.remaining > 0.0).count(),
            paper_closed_count: pv.closed_trades,
            updated_at_ms: s.updated_at_ms,
        }
    }
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(start_ms: u64) -> Candle {
        Candle {
            start_ms,
            end_ms: start_ms + 60_000,
            interval: "1".into(),
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
            turnover: 100.0,
            confirmed: true,
        }
    }

    #[test]
    fn delayed_backfill_stays_ordered_after_live_candle() {
        let mut state = InternalState::new();
        state.upsert_candle(candle(180_000));
        state.upsert_candle(candle(60_000));
        state.upsert_candle(candle(120_000));
        state.upsert_candle(candle(180_000));
        let times: Vec<_> = state.bars["1"].iter().map(|bar| bar.start_ms).collect();
        assert_eq!(times, [60_000, 120_000, 180_000]);
    }
}
