mod paper;
mod scanner;
mod catalog;
mod api;
mod bybit;
mod bybit_record;
mod config;
mod journal;
mod jev;
mod indicators;
mod flags;
mod live_recorder;
mod market_event;
mod microstructure;
mod outcomes;
mod replay;
mod replay_pipeline;
mod replay_simulator;
mod risk;
mod risk_store;
mod runtime;
mod signal;
mod simulator;
mod state;
mod types;
mod model;

use crate::{config::Config, state::AppState};
use std::{env, fs, io::Write, path::Path};
use tracing::info;
use tracing_subscriber::EnvFilter;

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let tmp = path.with_extension("tmp");
    { let mut file = fs::File::create(&tmp)?; file.write_all(bytes)?; file.sync_all()?; }
    fs::rename(tmp, path)?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let config = Config::from_env()?;
    if let Ok(output) = env::var("A6_REPLAY_REPORT_OUTPUT") {
        let input = env::var("A6_REPLAY_INPUT").unwrap_or_else(|_| config.market_record_path.display().to_string());
        let report = crate::replay_simulator::simulate_recording(Path::new(&input), crate::replay_simulator::ReplaySimulationConfig::default())?;
        atomic_write(Path::new(&output), &serde_json::to_vec_pretty(&report)?)?;
        info!(input=%input, output=%output, completed_round_trips=report.completed_round_trips, "offline replay simulation report written; live tasks were not started");
        return Ok(());
    }
    let state = AppState::new(config.clone())?;
    let api_state = state.clone();
    let feed_state = state.clone();
    let scanner_state = state.clone();
    let bind = config.bind_addr;
    let api = tokio::spawn(async move { api::serve(api_state, bind).await });
    let feed = tokio::spawn(async move { bybit::run(feed_state).await });
    let scanner = tokio::spawn(async move { scanner::run(scanner_state).await });
    tokio::select! {
        result = api => result??,
        result = feed => result??,
        result = scanner => result??,
    }
    Ok(())
}
