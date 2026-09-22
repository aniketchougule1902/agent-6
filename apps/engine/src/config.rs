use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub symbol: String,
    pub bybit_testnet: bool,
    pub http_addr: SocketAddr,
    pub timeframe: String,
    pub min_signal_score: f64,
    pub min_rr: f64,
    pub signal_cooldown_secs: u64,
    pub max_spread_bps: f64,
    pub stale_feed_ms: u64,
    pub journal_path: PathBuf,
    pub market_record_path: PathBuf,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();
        Ok(Self {
            symbol: env::var("A6_SYMBOL").unwrap_or_else(|_| "BTCUSDT".into()).to_uppercase(),
            bybit_testnet: parse_bool("A6_BYBIT_TESTNET", false),
            http_addr: env::var("A6_HTTP_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:8787".into())
                .parse()?,
            timeframe: env::var("A6_TIMEFRAME").unwrap_or_else(|_| "3".into()),
            min_signal_score: parse("A6_MIN_SIGNAL_SCORE", 0.74),
            min_rr: parse("A6_MIN_RR", 1.8),
            signal_cooldown_secs: parse("A6_SIGNAL_COOLDOWN_SECS", 90),
            max_spread_bps: parse("A6_MAX_SPREAD_BPS", 4.0),
            stale_feed_ms: parse("A6_STALE_FEED_MS", 3500),
            journal_path: env::var("A6_JOURNAL_PATH")
                .unwrap_or_else(|_| "data/journal.jsonl".into())
                .into(),
            market_record_path: env::var("A6_MARKET_RECORD_PATH")
                .unwrap_or_else(|_| "data/market-events.jsonl".into())
                .into(),
        })
    }

    pub fn public_ws_url(&self) -> &'static str {
        if self.bybit_testnet {
            "wss://stream-testnet.bybit.com/v5/public/linear"
        } else {
            "wss://stream.bybit.com/v5/public/linear"
        }
    }

    pub fn rest_base_url(&self) -> &'static str {
        if self.bybit_testnet {
            "https://api-testnet.bybit.com"
        } else {
            "https://api.bybit.com"
        }
    }
}

fn parse<T: std::str::FromStr + Copy>(key: &str, default: T) -> T {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn parse_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}
