use crate::{
    runtime::{self, RuntimeMarketConfig, RuntimeMarketUpdate},
    state::AppState,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;
use std::time::Duration;
use tokio::time;
use tower_http::cors::CorsLayer;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/state", get(snapshot))
        .route("/api/market", get(market_config).post(update_market))
        .route("/ws", get(ws_upgrade))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let snapshot = state.snapshot();
    Json(json!({
        "ok": true,
        "connected": snapshot.connected,
        "symbol": snapshot.symbol,
        "updated_at_ms": snapshot.updated_at_ms
    }))
}

async fn snapshot(State(state): State<AppState>) -> Json<crate::types::EngineSnapshot> {
    Json(state.snapshot())
}

async fn market_config() -> Json<RuntimeMarketConfig> {
    Json(runtime::current())
}

async fn update_market(
    State(state): State<AppState>,
    Json(request): Json<RuntimeMarketUpdate>,
) -> Result<Json<RuntimeMarketConfig>, (StatusCode, Json<serde_json::Value>)> {
    let previous = runtime::current();
    let next = runtime::update(request).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error.to_string()})),
        )
    })?;

    if next.symbol != previous.symbol {
        state.inner.write().reset_market();
    } else if next.timeframe != previous.timeframe {
        state.inner.write().reset_signal_context();
    }

    state.publish(crate::types::EngineEvent {
        ts_ms: crate::state::now_ms(),
        event_type: "market_update_requested".into(),
        alert: None,
        message: format!(
            "Requested {} on {}m (generation {}).",
            next.symbol, next.timeframe, next.generation
        ),
        signal: None,
    });

    Ok(Json(next))
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_loop(socket, state))
}

async fn ws_loop(mut socket: WebSocket, state: AppState) {
    let mut events = state.events.subscribe();
    let initial = json!({"kind":"snapshot","data":state.snapshot()}).to_string();
    if socket.send(Message::Text(initial.into())).await.is_err() {
        return;
    }

    let mut snapshots = time::interval(Duration::from_millis(750));
    loop {
        tokio::select! {
            _ = snapshots.tick() => {
                let payload = json!({"kind":"snapshot","data":state.snapshot()}).to_string();
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            event = events.recv() => {
                let Ok(event) = event else { continue; };
                let payload = json!({"kind":"event","data":event}).to_string();
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}
