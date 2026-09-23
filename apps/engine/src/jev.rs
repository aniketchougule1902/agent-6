use crate::{state::AppState, types::EngineEvent};
use serde_json::{json, Value};
use std::{env, time::Duration};
use tokio::time;
use tracing::warn;

/// Optional, asynchronous setup review. This is research evidence, never an
/// execution gate or a calibrated probability of a profitable trade.
pub async fn run(state: AppState) {
    let Ok(api_key) = env::var("TYPESAFE_API_KEY") else {
        return;
    };
    if api_key.trim().is_empty() {
        return;
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            warn!(?error, "Jev client unavailable");
            return;
        }
    };
    let mut last_id = String::new();
    let mut ticker = time::interval(Duration::from_secs(2));
    loop {
        ticker.tick().await;
        let snapshot = state.snapshot();
        let (Some(signal), Some(features)) = (snapshot.active_signal, snapshot.features) else {
            continue;
        };
        if signal.id == last_id || snapshot.feed_stale {
            continue;
        }
        last_id = signal.id.clone();

        let payload = json!({
            "model": "jev-latest",
            "state": {
                "side": signal.side,
                "timeframe_minutes": signal.timeframe,
                "quality_score_uncalibrated": signal.confidence,
                "regime": features.regime,
                "trend_5m_bps": features.trend_5m_bps,
                "trend_15m_bps": features.trend_15m_bps,
                "momentum_1m_bps": features.momentum_1m_bps,
                "spread_bps": features.spread_bps,
                "book_imbalance": features.book_imbalance,
                "trade_flow_imbalance": features.trade_flow_imbalance,
                "liquidation_pressure": features.liquidation_pressure
            },
            "questions": {
                "confluence": {
                    "type": "choice",
                    "instructions": "Do the listed observations qualitatively support the proposed direction at this instant? Classify only the internal consistency of the observations. Do not predict profit or treat the quality score as evidence.",
                    "criteria": {
                        "aligned": "Trend, short momentum and flow broadly support the proposed side.",
                        "mixed": "The observations conflict or are too weak to judge.",
                        "contradictory": "Several observations oppose the proposed side."
                    }
                }
            }
        });
        let answer = client
            .post("https://api.typesafe.ai/v1/systemone")
            .bearer_auth(&api_key)
            .json(&payload)
            .send()
            .await;
        let review = match answer {
            Ok(response) if response.status().is_success() => {
                match response.json::<Value>().await {
                    Ok(value) => value,
                    Err(error) => {
                        warn!(?error, "Jev response could not be parsed");
                        continue;
                    }
                }
            }
            Ok(response) => {
                warn!(status = %response.status(), "Jev review unavailable");
                continue;
            }
            Err(error) => {
                warn!(?error, "Jev review unavailable");
                continue;
            }
        };
        let Some(choice) = review
            .pointer("/answers/confluence/choice")
            .and_then(Value::as_str)
        else {
            continue;
        };
        if !["aligned", "mixed", "contradictory"].contains(&choice) {
            continue;
        }
        let confidence = review
            .pointer("/answers/confluence/confidence")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value));
        // Do not attach a review to a newer market or altered setup.
        let current = state.snapshot();
        if current.symbol != signal.symbol
            || current.active_signal.as_ref().map(|s| &s.id) != Some(&signal.id)
        {
            continue;
        }
        state.publish(EngineEvent {
            ts_ms: crate::state::now_ms(),
            event_type: "jev_review".into(),
            alert: None,
            message: format!("Jev confluence review: {choice} (model certainty: {}); research-only, not a win probability", confidence.map(|c| format!("{:.0}%", c * 100.0)).unwrap_or_else(|| "unknown".into())),
            signal: Some(signal),
        });
    }
}
