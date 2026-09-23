use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Side {
    Long,
    Short,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SignalStatus {
    Active,
    Tp1Hit,
    Tp2Hit,
    StopLossHit,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AlertKind {
    Signal,
    Tp1,
    Tp2,
    StopLoss,
    Expired,
    FeedDisconnected,
    FeedReconnected,
    FeedStale,
    FeedRecovered,
    Drift,
    ModelPromoted,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Candle {
    pub start_ms: u64,
    pub end_ms: u64,
    pub interval: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub turnover: f64,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MarketRegime {
    TrendingUp,
    TrendingDown,
    Range,
    HighVolatility,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FeatureSnapshot {
    pub ts_ms: u64,
    pub feed_age_ms: u64,
    pub orderbook_age_ms: u64,
    pub last_price: f64,
    pub spread_bps: f64,
    pub atr_14: f64,
    pub vwap_20: f64,
    pub momentum_1m_bps: f64,
    pub trend_5m_bps: f64,
    pub trend_15m_bps: f64,
    pub book_imbalance: f64,
    pub book_imbalance_top5: f64,
    pub microprice_bps: f64,
    pub bid_depth_slope: f64,
    pub ask_depth_slope: f64,
    pub depth_pressure: f64,
    pub trade_flow_imbalance: f64,
    pub trade_velocity_5s: f64,
    pub signed_notional_5s: f64,
    pub large_trade_imbalance: f64,
    pub liquidation_pressure: f64,
    pub liquidation_burst_5s: f64,
    pub open_interest: Option<f64>,
    pub open_interest_delta_pct: f64,
    pub open_interest_delta_1m_pct: f64,
    pub open_interest_delta_5m_pct: f64,
    pub funding_rate: Option<f64>,
    pub regime: MarketRegime,
    pub long_score: f64,
    pub short_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TradeSignal {
    pub id: String,
    pub symbol: String,
    pub timeframe: String,
    pub side: Side,
    pub status: SignalStatus,
    pub created_at_ms: u64,
    pub entry_low: f64,
    pub entry_high: f64,
    pub stop_loss: f64,
    pub tp1: f64,
    pub tp2: f64,
    pub risk_reward_tp2: f64,
    pub confidence: f64,
    pub calibrated: bool,
    pub invalidation: String,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub last_event_ms: u64,
    #[serde(default)]
    pub observed_exit_price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimeframeAnalysis {
    pub timeframe: String,
    pub candle_ms: u64,
    pub close: f64,
    pub ema9: f64,
    pub ema21: f64,
    pub ema50: f64,
    pub rsi14: f64,
    pub adx14: f64,
    pub macd_histogram: f64,
    pub atr14: f64,
    pub vwap20: f64,
    pub bb_upper: f64,
    pub bb_lower: f64,
    pub relative_volume: f64,
    pub support: f64,
    pub resistance: f64,
    pub bias: String,
    pub setup: String,
    pub quality: f64,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChartFlag {
    pub id: String,
    pub symbol: String,
    pub timeframe: String,
    pub ts_ms: u64,
    pub price: f64,
    pub side: Side,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EngineEvent {
    pub ts_ms: u64,
    pub event_type: String,
    pub alert: Option<AlertKind>,
    pub message: String,
    pub signal: Option<TradeSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EngineSnapshot {
    pub symbol: String,
    pub timeframe: String,
    pub market_generation: u64,
    pub connected: bool,
    pub feed_stale: bool,
    pub feed_age_ms: u64,
    pub last_price: Option<f64>,
    pub mark_price: Option<f64>,
    pub index_price: Option<f64>,
    pub features: Option<FeatureSnapshot>,
    pub active_signal: Option<TradeSignal>,
    pub timeframe_signals: Vec<TradeSignal>,
    pub analyses: Vec<TimeframeAnalysis>,
    pub chart_flags: Vec<ChartFlag>,
    pub candles_1m: Vec<Candle>,
    pub candles_3m: Vec<Candle>,
    pub candles_5m: Vec<Candle>,
    pub candles_15m: Vec<Candle>,
    pub updated_at_ms: u64,
}
