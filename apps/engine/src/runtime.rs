use crate::config::Config;
use anyhow::{bail, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tokio::sync::watch;
use ts_rs::TS;

const ALLOWED_TIMEFRAMES: &[&str] = &["1", "3", "5", "15"];

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct RuntimeMarketConfig {
    pub symbol: String,
    pub timeframe: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeMarketUpdate {
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
}

struct RuntimeMarket {
    config: RwLock<RuntimeMarketConfig>,
    changes: watch::Sender<RuntimeMarketConfig>,
}

static RUNTIME: OnceLock<RuntimeMarket> = OnceLock::new();

pub fn init(config: &Config) -> Result<()> {
    validate_symbol(&config.symbol)?;
    validate_timeframe(&config.timeframe)?;
    let initial = RuntimeMarketConfig {
        symbol: config.symbol.clone(),
        timeframe: config.timeframe.clone(),
        generation: 1,
    };
    let (changes, _receiver) = watch::channel(initial.clone());
    RUNTIME
        .set(RuntimeMarket {
            config: RwLock::new(initial),
            changes,
        })
        .map_err(|_| anyhow::anyhow!("runtime market configuration already initialized"))
}

pub fn current() -> RuntimeMarketConfig {
    RUNTIME
        .get()
        .expect("runtime market configuration not initialized")
        .config
        .read()
        .clone()
}

pub fn subscribe() -> watch::Receiver<RuntimeMarketConfig> {
    RUNTIME
        .get()
        .expect("runtime market configuration not initialized")
        .changes
        .subscribe()
}

pub fn update(request: RuntimeMarketUpdate) -> Result<RuntimeMarketConfig> {
    let runtime = RUNTIME
        .get()
        .expect("runtime market configuration not initialized");
    let previous = runtime.config.read().clone();

    let symbol = request
        .symbol
        .map(|value| value.trim().to_ascii_uppercase())
        .unwrap_or_else(|| previous.symbol.clone());
    let timeframe = request
        .timeframe
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| previous.timeframe.clone());

    validate_symbol(&symbol)?;
    validate_timeframe(&timeframe)?;

    if previous.symbol == symbol && previous.timeframe == timeframe {
        return Ok(previous);
    }

    let next = RuntimeMarketConfig {
        symbol,
        timeframe,
        generation: previous.generation.saturating_add(1),
    };
    *runtime.config.write() = next.clone();
    runtime.changes.send_replace(next.clone());
    Ok(next)
}

fn validate_symbol(symbol: &str) -> Result<()> {
    if symbol.len() < 5
        || symbol.len() > 24
        || !symbol.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        bail!("symbol must be 5-24 uppercase ASCII letters/digits, e.g. BTCUSDT");
    }
    Ok(())
}

fn validate_timeframe(timeframe: &str) -> Result<()> {
    if !ALLOWED_TIMEFRAMES.contains(&timeframe) {
        bail!("timeframe must be one of 1, 3, 5, 15 minutes");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_runtime_values() {
        assert!(validate_symbol("../BAD").is_err());
        assert!(validate_symbol("BTCUSDT").is_ok());
        assert!(validate_timeframe("2").is_err());
        assert!(validate_timeframe("15").is_ok());
    }
}
