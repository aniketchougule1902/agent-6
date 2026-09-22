use crate::{
    config::Config,
    journal::Journal,
    types::{Candle, EngineEvent, EngineSnapshot, FeatureSnapshot, TradeSignal},
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
    pub features: Option<FeatureSnapshot>,
    pub active_signal: Option<TradeSignal>,
    pub last_signal_at_ms: u64,
    pub updated_at_ms: u64,
}

impl InternalState {
    fn new() -> Self {
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
            features: None,
            active_signal: None,
            last_signal_at_ms: 0,
            updated_at_ms: now,
        }
    }

    pub fn upsert_candle(&mut self, candle: Candle) {
        let deque = self.bars.entry(candle.interval.clone()).or_default();
        if let Some(existing) = deque.iter_mut().find(|c| c.start_ms == candle.start_ms) {
            *existing = candle;
        } else {
            deque.push_back(candle);
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

    pub fn prune_flows(&mut self, now: u64) {
        let min_trade_ts = now.saturating_sub(20_000);
        let min_liq_ts = now.saturating_sub(60_000);
        while self.trade_flow.front().is_some_and(|x| x.ts_ms < min_trade_ts) {
            self.trade_flow.pop_front();
        }
        while self.liquidation_flow.front().is_some_and(|x| x.ts_ms < min_liq_ts) {
            self.liquidation_flow.pop_front();
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub(crate) inner: Arc<RwLock<InternalState>>,
    pub events: broadcast::Sender<EngineEvent>,
    journal: Arc<Journal>,
}

impl AppState {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let (events, _) = broadcast::channel(512);
        Ok(Self {
            journal: Arc::new(Journal::open(&config.journal_path)?),
            config,
            inner: Arc::new(RwLock::new(InternalState::new())),
            events,
        })
    }

    pub fn publish(&self, event: EngineEvent) {
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
        EngineSnapshot {
            symbol: self.config.symbol.clone(),
            connected: s.connected,
            feed_stale: s.feed_stale,
            feed_age_ms: now.saturating_sub(s.last_market_event_ms),
            last_price: s.last_price,
            mark_price: s.mark_price,
            index_price: s.index_price,
            features: s.features.clone(),
            active_signal: s.active_signal.clone(),
            candles_1m: bars("1"),
            candles_3m: bars("3"),
            candles_5m: bars("5"),
            candles_15m: bars("15"),
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
