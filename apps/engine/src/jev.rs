use crate::{
    state::{now_ms, AppState},
    types::{EngineEvent, SignalStatus},
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashSet, sync::OnceLock, time::Duration};
#[derive(Clone, Serialize, Default)]
pub struct Health {
    pub status: String,
    pub last_attempt_ms: u64,
    pub last_success_ms: u64,
    pub requests: u64,
    pub last_review: String,
    pub error: Option<String>,
}
static HEALTH: OnceLock<parking_lot::RwLock<Health>> = OnceLock::new();
fn health() -> &'static parking_lot::RwLock<Health> {
    HEALTH.get_or_init(|| {
        parking_lot::RwLock::new(Health {
            status: "starting".into(),
            ..Health::default()
        })
    })
}
pub fn status() -> Health {
    let mut h = health().read().clone();
    if h.last_success_ms > 0
        && now_ms().saturating_sub(h.last_success_ms) > 600_000
        && h.status == "online"
    {
        h.status = "idle; awaiting fresh review".into();
    }
    h
}
pub async fn run(state: AppState) {
    let key = std::env::var("TYPESAFE_API_KEY").unwrap_or_default();
    if key.trim().is_empty() {
        health().write().status = "not configured".into();
        return;
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            health().write().status = "client unavailable".into();
            return;
        }
    };
    let mut seen = HashSet::new();
    let mut next_context = 0;
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let snap = state.snapshot();
        if !snap.connected || snap.feed_stale {
            continue;
        }
        let Some(f) = snap.features else { continue };
        let signal = snap
            .timeframe_signals
            .iter()
            .find(|s| {
                matches!(s.status, SignalStatus::Active | SignalStatus::Tp1Hit)
                    && !seen.contains(&s.id)
            })
            .cloned();
        if signal.is_none() && now_ms() < next_context {
            continue;
        }
        let input = json!({"symbol":snap.symbol,"setup":signal.as_ref().map(|s|json!({"side":s.side,"timeframe":s.timeframe,"reasons":s.reasons})),"regime":f.regime,"trend5_bps":f.trend_5m_bps,"trend15_bps":f.trend_15m_bps,"flow":f.trade_flow_imbalance,"book":f.book_imbalance_top5,"spread_bps":f.spread_bps});
        let payload = json!({"model":"jev-latest","state":input,"questions":{"confluence":{"type":"choice","instructions":"Classify internal directional consistency. If a setup is supplied assess its side against trend and flow; otherwise assess whether trend and flow agree. Do not predict profit or authorize orders.","criteria":{"aligned":"Trends and flow support a consistent direction and the supplied setup side if any.","mixed":"Weak or conflicting observations.","contradictory":"Several observations oppose the supplied setup direction."}}}});
        {
            let mut h = health().write();
            h.last_attempt_ms = now_ms();
            h.requests += 1;
            h.status = "reviewing".into();
        }
        let result = async {
            let r = client
                .post("https://api.typesafe.ai/v1/systemone")
                .bearer_auth(&key)
                .json(&payload)
                .send()
                .await
                .map_err(|_| "Network error contacting Jev".to_string())?;
            if !r.status().is_success() {
                return Err(format!("Jev HTTP {}", r.status().as_u16()));
            }
            let v = r
                .json::<Value>()
                .await
                .map_err(|_| "Invalid Jev JSON".to_string())?;
            let choice = v
                .pointer("/answers/confluence/choice")
                .and_then(Value::as_str)
                .filter(|c| ["aligned", "mixed", "contradictory"].contains(c))
                .ok_or("Invalid Jev choice")?;
            Ok::<String, String>(choice.into())
        }
        .await;
        next_context = now_ms() + 300_000;
        match result {
            Ok(choice) => {
                let message = format!(
                    "{} {}: {choice}; qualitative review, not a win probability",
                    snap.symbol,
                    signal
                        .as_ref()
                        .map(|s| format!("{}m", s.timeframe))
                        .unwrap_or_else(|| "market context".into())
                );
                {
                    let mut h = health().write();
                    h.status = "online".into();
                    h.last_success_ms = now_ms();
                    h.last_review = message.clone();
                    h.error = None;
                }
                if let Some(s) = &signal {
                    seen.insert(s.id.clone());
                }
                if seen.len() > 1000 {
                    seen.clear();
                }
                state.publish(EngineEvent {
                    ts_ms: now_ms(),
                    event_type: "jev_review".into(),
                    alert: None,
                    message,
                    signal,
                });
            }
            Err(error) => {
                {
                    let mut h = health().write();
                    h.status = "unavailable".into();
                    h.error = Some(error);
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
                next_context = 0;
            }
        }
    }
}
