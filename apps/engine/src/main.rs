mod paper;
mod demo;
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
mod microstructure;
mod outcomes;
mod replay;
mod risk;
mod runtime;
mod signal;
mod simulator;
mod state;
mod types;
mod model;

use crate::{config::Config, state::AppState};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("agent6_engine=info")),
        )
        .init();

    let config = Config::from_env()?;
    runtime::init(&config)?;
    let state = AppState::new(config.clone())?;

    let testnet=config.bybit_testnet;
    tokio::spawn(async move { loop { let _=catalog::get(testnet).await; tokio::time::sleep(std::time::Duration::from_secs(60)).await; } });
    tokio::spawn(bybit::run_forever(state.clone()));
    tokio::spawn(signal::run_loop(state.clone()));
    tokio::spawn(jev::run(state.clone()));
    tokio::spawn(paper::run(state.clone()));

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
