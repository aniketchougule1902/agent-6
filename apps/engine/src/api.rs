use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
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
