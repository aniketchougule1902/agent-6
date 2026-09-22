use crate::config::Config;
use anyhow::{bail, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};
use ts_rs::TS;

const ALLOWED_TIMEFRAMES: &[&str] = &["1", "3", "5", "15"];

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct RuntimeMarketConfig {
    pub symbol: String,
    pub timeframe: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeMarketUpdate {
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
}

struct RuntimeMarket {
    symbol: RwLock<String>,
    timeframe: RwLock<String>,
    generation: AtomicU64,
}

static RUNTIME: OnceLock<RuntimeMarket> = OnceLock::new();

pub fn init(config: &Config) -> Result<()> {
    validate_symbol(&config.symbol)?;
    validate_timeframe(&config.timeframe)?;
    RUNTIME
        .set(RuntimeMarket {
            symbol: RwLock::new(config.symbol.clone()),
            timeframe: RwLock::new(config.timeframe.clone()),
            generation: AtomicU64::new(1),
        })
        .map_err(|_| anyhow::anyhow!("runtime market configuration already initialized"))
}

pub fn current() -> RuntimeMarketConfig {
    let runtime = RUNTIME.get().expect("runtime market configuration not initialized");
    RuntimeMarketConfig {
        symbol: runtime.symbol.read().clone(),
        timeframe: runtime.timeframe.read().clone(),
        generation: runtime.generation.load(Ordering::Acquire),
    }
}

pub fn update(request: RuntimeMarketUpdate) -> Result<RuntimeMarketConfig> {
    let runtime = RUNTIME.get().expect("runtime market configuration not initialized");
    let current = current();
    let symbol = request
        .symbol
        .map(|value| value.trim().to_ascii_uppercase())
        .unwrap_or(current.symbol);
    let timeframe = request
        .timeframe
        .map(|value| value.trim().to_string())
        .unwrap_or(current.timeframe);

    validate_symbol(&symbol)?;
    validate_timeframe(&timeframe)?;

    let changed = *runtime.symbol.read() != symbol || *runtime.timeframe.read() != timeframe;
    if changed {
        *runtime.symbol.write() = symbol;
        *runtime.timeframe.write() = timeframe;
        runtime.generation.fetch_add(1, Ordering::AcqRel);
    }
    Ok(current())
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
