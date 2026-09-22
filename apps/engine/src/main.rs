mod api;
mod bybit;
mod config;
mod journal;
mod microstructure;
mod outcomes;
mod replay;
mod runtime;
mod signal;
mod simulator;
mod state;
mod types;

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

    tokio::spawn(bybit::run_forever(state.clone()));
    tokio::spawn(signal::run_loop(state.clone()));

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
