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
    routing::{get,post},
    Json, Router,
};
use serde_json::json;
use std::time::Duration;
use tokio::time;
use tower_http::cors::CorsLayer;

pub fn router(state: AppState) -> Router {
    Router::new()
         .route("/api/health", get(health))
        .route("/api/paper",get(paper_view))
        .route("/api/paper/enter",post(paper_enter))
        .route("/api/paper/close",post(paper_close))
        .route("/api/paper/reset",post(paper_reset))
        .route("/api/demo/win", get(|| async { demo_result(true) }))
        .route("/api/demo/loss", get(|| async { demo_result(false) }))
        .route("/api/instruments", get(instruments))
        .route("/api/state", get(snapshot))
        .route("/api/market", get(market_config).post(update_market))
        .route("/ws", get(ws_upgrade))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn instruments(State(state): State<AppState>) -> Json<crate::catalog::Catalog> {
    Json(crate::catalog::get(state.config.bybit_testnet).await)
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let snapshot = state.snapshot();
    Json(json!({
        "ok": snapshot.connected && !snapshot.feed_stale && snapshot.features.is_some(),
        "connected": snapshot.connected,
        "services": {
            "market_feed": if snapshot.connected && !snapshot.feed_stale {"live"} else {"unavailable"},
            "signal_engine": if snapshot.analyses.len()==4 && !snapshot.feed_stale {"evaluating"} else {"warming / halted"},
            "history": if snapshot.analyses.len()==4 {"ready"} else {"loading"},
            "jev": if std::env::var("TYPESAFE_API_KEY").is_ok_and(|v| !v.trim().is_empty()) {"configured; review on signal"} else {"not configured"},
            "execution": "paper only",
            "calibrated_model": "not deployed"
        },
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
    if let Some(symbol) = &request.symbol {
        let catalog=crate::catalog::get(state.config.bybit_testnet).await;
        if catalog.instruments.is_empty() {
            return Err((StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"Cannot validate symbol: exchange catalog unavailable"}))));
        }
        if !catalog.instruments.iter().any(|i|i.symbol==symbol.trim().to_ascii_uppercase()) {
            return Err((StatusCode::BAD_REQUEST,Json(json!({"error":"Symbol is not an active Bybit linear perpetual. Select a ticker from search."}))));
        }
    }
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

fn demo_result(win:bool)->Result<Json<crate::demo::Demo>,(StatusCode,Json<serde_json::Value>)>{
 crate::demo::run(win).map(Json).map_err(|_|(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":"Demo simulation failed"}))))
}

#[derive(serde::Deserialize)]struct PaperEntry {signal_id:String,notional:f64}
#[derive(serde::Deserialize)]struct PaperClose {position_id:String}
#[derive(serde::Deserialize)]struct PaperReset {confirmation:String,revision:u64}
type ApiError=(StatusCode,Json<serde_json::Value>);
fn paper_error(e:impl ToString)->ApiError{(StatusCode::BAD_REQUEST,Json(json!({"error":e.to_string()})))}
async fn paper_view(State(state):State<AppState>)->Json<crate::paper::View>{Json(state.paper.lock().view())}
async fn paper_enter(State(state):State<AppState>,Json(request):Json<PaperEntry>)->Result<Json<crate::paper::View>,ApiError>{
 let market=runtime::current();let inner=state.inner.read();
 if !inner.connected||inner.feed_stale||crate::state::now_ms().saturating_sub(inner.orderbook_event_ms)>state.config.stale_feed_ms{return Err(paper_error("Fresh selected-market feed required"));}
 let signal=inner.signals.values().find(|s|s.id==request.signal_id&&s.symbol==market.symbol).ok_or_else(||paper_error("Signal no longer available"))?;
 let bid=inner.bid.ok_or_else(||paper_error("No bid"))?;let ask=inner.ask.ok_or_else(||paper_error("No ask"))?;
 let mut paper=state.paper.lock();paper.enter(signal,request.notional,bid,ask,crate::state::now_ms()).map_err(paper_error)?;Ok(Json(paper.view()))
}
async fn paper_close(State(state):State<AppState>,Json(request):Json<PaperClose>)->Result<Json<crate::paper::View>,ApiError>{
 let symbol={let paper=state.paper.lock();paper.account.positions.iter().find(|p|p.id==request.position_id&&p.remaining>0.0).map(|p|p.symbol.clone()).ok_or_else(||paper_error("Open position not found"))?};
 let quotes=crate::paper::quotes(state.config.bybit_testnet).await.map_err(paper_error)?;
 let quote=quotes.get(&symbol).ok_or_else(||paper_error("No fresh price for this symbol"))?;
 let mut paper=state.paper.lock();paper.close(&request.position_id,quote.bid,quote.ask,quote.ts).map_err(paper_error)?;Ok(Json(paper.view()))
}
async fn paper_reset(State(state):State<AppState>,Json(request):Json<PaperReset>)->Result<Json<crate::paper::View>,ApiError>{
 let mut paper=state.paper.lock();if request.confirmation!="RESET"||request.revision!=paper.account.revision{return Err(paper_error("Account changed or reset unconfirmed; refresh and retry"));}
 paper.reset().map_err(paper_error)?;Ok(Json(paper.view()))
}
