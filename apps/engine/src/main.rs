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
mod multi_venue;
mod venue_context;
mod venue_health;
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

/// Offline-only evaluation mode. When A6_REPLAY_REPORT_OUTPUT is set, the
/// engine reads the normalized production recording, drives the exact Rust
/// replay -> signal lifecycle -> realistic simulator path, atomically writes a
/// JSON report, and exits before any live websocket/paper tasks are started.
fn maybe_run_replay_report(config: &Config) -> anyhow::Result<bool> {
    let Ok(output) = env::var("A6_REPLAY_REPORT_OUTPUT") else { return Ok(false); };
    anyhow::ensure!(!output.trim().is_empty(), "A6_REPLAY_REPORT_OUTPUT cannot be empty");
    let input = env::var("A6_REPLAY_INPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| config.market_record_path.clone());
    anyhow::ensure!(input.is_file(), "normalized replay input does not exist: {}", input.display());

    let events = replay::read_recording(&input)?;
    replay::validate_feed_integrity(&events)?;
    anyhow::ensure!(!events.is_empty(), "normalized replay input is empty");
    let market = runtime::current();
    let report = replay_simulator::run_replay_simulation(
        config,
        &market,
        events,
        simulator::SimulationConfig::default(),
    )?;

    let output = Path::new(&output);
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temp = output.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp)?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(&temp, output)?;
    info!(input=%input.display(), output=%output.display(), round_trips=report.completed_round_trips,
        net_pnl_quote=report.net_pnl_quote, "offline replay after-cost report written");
    Ok(true)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("agent6_engine=info")),
        )
        .init();

    let config = Config::from_env()?;
    anyhow::ensure!(config.http_addr.ip().is_loopback(),"This paper terminal binds only to loopback; remote deployment requires authentication and TLS");
    runtime::init(&config)?;
    if maybe_run_replay_report(&config)? {
        return Ok(());
    }
    let state = AppState::new(config.clone())?;

    let testnet=config.bybit_testnet;
    tokio::spawn(async move { loop { let _=catalog::get(testnet).await; tokio::time::sleep(std::time::Duration::from_secs(60)).await; } });
    tokio::spawn(bybit::run_forever(state.clone()));
    tokio::spawn(signal::run_loop(state.clone()));
    tokio::spawn(jev::run(state.clone()));
    tokio::spawn(paper::run(state.clone()));
    tokio::spawn(scanner::run(state.clone()));
    tokio::spawn(model::watch(state.clone()));

    let listener = tokio::net::TcpListener::bind(config.http_addr).await?;
    let market = runtime::current();
    info!(
        addr = %config.http_addr,
        symbol = %market.symbol,
        timeframe = %market.timeframe,
        testnet = config.bybit_testnet,
        "Agent-6 local engine started"
    );

    axum::serve(listener, api::router(state)).await?;
    Ok(())
}
