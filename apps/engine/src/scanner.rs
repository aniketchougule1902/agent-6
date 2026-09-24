use crate::{
    state::{now_ms, AppState},
    types::{Candle, TimeframeAnalysis},
};
use futures_util::{stream, StreamExt};
use serde::Serialize;
const UNIVERSE_SIZE: usize = 200;
use serde_json::Value;
use std::{collections::VecDeque, sync::OnceLock, time::Duration};
#[derive(Clone, Serialize)]
pub struct ScanRow {
    pub symbol: String,
    pub price: f64,
    pub turnover24h: f64,
    /// 1-based position inside this scan cycle's 200-market liquidity universe.
    /// This is deliberately separate from setup-quality rank.
    pub universe_rank: usize,
    pub change24h: f64,
    pub checked_ms: u64,
    /// Composite radar score. This is a setup-quality score, not a win probability.
    pub quality: f64,
    pub base_quality: f64,
    pub htf_quality: f64,
    pub spread_bps: f64,
    pub displacement_atr: f64,
    pub execution_quality: f64,
    pub stability_cycles: u32,
    pub last_qualified_ms: u64,
    pub peak_quality: f64,
    pub phase: String,
    pub side: String,
    pub setup: String,
    pub blockers: Vec<String>,
    pub entry: Option<f64>,
    pub stop: Option<f64>,
    pub tp1: Option<f64>,
    pub tp2: Option<f64>,
    pub first_seen_ms: u64,
}
#[derive(Clone, Default, Serialize)]
pub struct Scanner {
    pub status: String,
    pub cycle_started_ms: u64,
    pub completed_ms: u64,
    pub scanning_symbol: String,
    pub scanned: usize,
    pub rows: Vec<ScanRow>,
    pub error: Option<String>,
}
static SCAN: OnceLock<parking_lot::RwLock<Scanner>> = OnceLock::new();
fn store() -> &'static parking_lot::RwLock<Scanner> {
    SCAN.get_or_init(|| parking_lot::RwLock::new(Scanner::default()))
}
pub fn snapshot() -> Scanner {
    store().read().clone()
}

fn execution_quality(spread_bps: f64, max_spread_bps: f64, displacement_atr: f64) -> f64 {
    if !spread_bps.is_finite() || !displacement_atr.is_finite() || max_spread_bps <= 0.0 {
        return 0.0;
    }
    let spread = (1.0 - spread_bps / max_spread_bps).clamp(0.0, 1.0);
    let entry = (1.0 - displacement_atr / 0.75).clamp(0.0, 1.0);
    0.60 * spread + 0.40 * entry
}

fn radar_score(base_quality: f64, htf_quality: f64, execution: f64, stability_cycles: u32) -> f64 {
    let stability = (stability_cycles.min(3) as f64 / 3.0).clamp(0.0, 1.0);
    (0.50 * base_quality.clamp(0.0, 1.0)
        + 0.25 * htf_quality.clamp(0.0, 1.0)
        + 0.15 * execution.clamp(0.0, 1.0)
        + 0.10 * stability)
        .clamp(0.0, 1.0)
}

async fn request(
    client: &reqwest::Client,
    testnet: bool,
    path: &str,
    params: &[(&str, &str)],
) -> anyhow::Result<Value> {
    let hosts = if testnet {
        vec!["https://api-testnet.bybit.com"]
    } else {
        vec!["https://api.bytick.com", "https://api.bybit.com"]
    };
    for host in hosts {
        if let Ok(response) = client
            .get(format!("{host}{path}"))
            .query(params)
            .send()
            .await
        {
            if let Ok(v) = response.json::<Value>().await {
                if v["retCode"] == 0 {
                    return Ok(v);
                }
            }
        }
    }
    anyhow::bail!("Exchange scan request unavailable")
}
async fn analysis(
    client: &reqwest::Client,
    testnet: bool,
    symbol: &str,
    tf: &str,
) -> anyhow::Result<TimeframeAnalysis> {
    let v = request(
        client,
        testnet,
        "/v5/market/kline",
        &[
            ("category", "linear"),
            ("symbol", symbol),
            ("interval", tf),
            ("limit", "150"),
        ],
    )
    .await?;
    let rows = v["result"]["list"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No candles"))?;
    let mut bars = Vec::new();
    for row in rows {
        let Some(r) = row.as_array() else { continue };
        if r.len() < 7 {
            continue;
        }
        let num = |i: usize| {
            r[i].as_str()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(f64::NAN)
        };
        let start = r[0]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        bars.push(Candle {
            start_ms: start,
            end_ms: start + tf.parse::<u64>()? * 60_000 - 1,
            interval: tf.into(),
            open: num(1),
            high: num(2),
            low: num(3),
            close: num(4),
            volume: num(5),
            turnover: num(6),
            confirmed: true,
        });
    }
    bars.sort_by_key(|b| b.start_ms);
    let bars: VecDeque<_> = bars.into();
    crate::indicators::analyze(&bars, tf, now_ms())
        .ok_or_else(|| anyhow::anyhow!("Insufficient valid closed candles"))
}
pub async fn run(state: AppState) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap();
    loop {
        {
            let mut s = store().write();
            s.status = "scanning".into();
            s.cycle_started_ms = now_ms();
            s.scanned = 0;
            s.error = None;
        }
        let result = request(
            &client,
            state.config.bybit_testnet,
            "/v5/market/tickers",
            &[("category", "linear")],
        )
        .await;
        match result {
            Ok(v) => {
                let catalog = crate::catalog::get(state.config.bybit_testnet).await;
                let mut rows = Vec::new();
                if let Some(list) = v["result"]["list"].as_array() {
                    for t in list {
                        let symbol = t["symbol"].as_str().unwrap_or("");
                        if !catalog.instruments.iter().any(|i| {
                            i.symbol == symbol
                                && i.quote == "USDT"
                                && ["", "innovation"].contains(&i.asset_type.as_str())
                        }) {
                            continue;
                        }
                        let num = |k: &str| {
                            t[k].as_str()
                                .and_then(|s| s.parse::<f64>().ok())
                                .unwrap_or(0.0)
                        };
                        let last = num("lastPrice");
                        if !last.is_finite() || last <= 0.0 || !num("turnover24h").is_finite() || num("turnover24h") <= 0.0 {
                            continue;
                        }
                        rows.push((
                            ScanRow {
                                symbol: symbol.into(),
                                price: last,
                                turnover24h: num("turnover24h"),
                                universe_rank: 0,
                                change24h: num("price24hPcnt") * 100.0,
                                checked_ms: 0,
                                quality: 0.0,
                                base_quality: 0.0,
                                htf_quality: 0.0,
                                spread_bps: 0.0,
                                displacement_atr: 0.0,
                                execution_quality: 0.0,
                                stability_cycles: 0,
                                last_qualified_ms: 0,
                                peak_quality: 0.0,
                                phase: "waiting for scan".into(),
                                side: "neutral".into(),
                                setup: String::new(),
                                blockers: vec![],
                                entry: None,
                                stop: None,
                                tp1: None,
                                tp2: None,
                                first_seen_ms: 0,
                            },
                            num("bid1Price"),
                            num("ask1Price"),
                        ));
                    }
                }
                // Turnover chooses the broad liquid universe only. It must never decide
                // the displayed setup order; the UI ranks all qualified members by quality.
                rows.sort_by(|a, b| b.0.turnover24h.total_cmp(&a.0.turnover24h));
                rows.truncate(UNIVERSE_SIZE);
                for (index, (row, _, _)) in rows.iter_mut().enumerate() {
                    row.universe_rank = index + 1;
                }
                {
                    let mut s = store().write();
                    let old = s.rows.clone();
                    s.rows = rows
                        .iter()
                        .map(|(r, _, _)| {
                            if let Some(prev) = old.iter().find(|p| p.symbol == r.symbol) {
                                let mut kept = prev.clone();
                                kept.turnover24h = r.turnover24h;
                                kept.universe_rank = r.universe_rank;
                                kept.price = r.price;
                                kept.change24h = r.change24h;
                                kept
                            } else {
                                r.clone()
                            }
                        })
                        .collect();
                }
                let quote_observed_ms = store().read().cycle_started_ms;
                let mut completed = stream::iter(rows.into_iter().map(|(mut row, bid, ask)| {
                    let client = &client;
                    let config = &state.config;
                    async move {
                    let (a, b) = tokio::join!(
                        analysis(client, config.bybit_testnet, &row.symbol, "5"),
                        analysis(client, config.bybit_testnet, &row.symbol, "15")
                    );
                    match (a, b) {
                        (Ok(a), Ok(b)) => {
                            row.base_quality = a.quality;
                            row.htf_quality = b.quality;
                            row.side = a.bias.clone();
                            row.setup = a.setup.clone();
                            row.blockers = a.blockers;
                            if a.bias != b.bias || a.bias == "neutral" {
                                row.blockers.push("15m trend does not confirm".into());
                            }
                            let spread_valid = bid.is_finite() && ask.is_finite() && bid > 0.0 && ask >= bid;
                            row.spread_bps = if spread_valid {
                                (ask - bid) / row.price * 10000.0
                            } else {
                                config.max_spread_bps * 10.0
                            };
                            row.displacement_atr = if a.atr14 > 0.0 {
                                (row.price - a.close).abs() / a.atr14
                            } else {
                                10.0
                            };
                            row.execution_quality = execution_quality(
                                row.spread_bps,
                                config.max_spread_bps,
                                row.displacement_atr,
                            );
                            row.quality = radar_score(
                                row.base_quality,
                                row.htf_quality,
                                row.execution_quality,
                                1,
                            );
                            if !spread_valid || row.spread_bps > config.max_spread_bps {
                                row.blockers.push("Spread/liquidity check failed".into());
                            }
                            if row.displacement_atr > 0.75 {
                                row.blockers.push("Price moved beyond entry window".into());
                            }
                            if row.base_quality < config.min_signal_score {
                                row.blockers.push("Base setup quality below threshold".into());
                            }
                            if row.side != "neutral" {
                                let d = if row.side == "long" { 1.0 } else { -1.0 };
                                let risk = a.atr14 * 1.5;
                                let cost = row.price * config.round_trip_cost_bps / 10000.0;
                                if risk > 0.0
                                    && (risk * 2.5 - cost) / (risk + cost) >= config.min_rr
                                {
                                    row.entry = Some(row.price);
                                    row.stop = Some(row.price - d * risk);
                                    row.tp1 = Some(row.price + d * risk * 1.4);
                                    row.tp2 = Some(row.price + d * risk * 2.5);
                                } else {
                                    row.blockers.push("Costs fail estimated reward/risk".into());
                                }
                            }
                            row.phase = if row.blockers.is_empty() {
                                "confirming stability"
                            } else if row.setup != "waiting" && row.side != "neutral" {
                                "forming"
                            } else {
                                "watching"
                            }
                            .into();
                        }
                        _ => {
                            row.phase = "data unavailable".into();
                            row.blockers
                                .push("History request failed; no actionable candidate".into());
                        }
                    }
                    // Price/spread evidence comes from the cycle's ticker snapshot.
                    // Never stamp old quotes as fresh when a slow history request completes.
                    row.checked_ms = quote_observed_ms;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    row
                    }
                })).buffer_unordered(4);
                while let Some(mut row) = completed.next().await {
                    {
                        let mut s = store().write();
                        if let Some(old) = s.rows.iter_mut().find(|r| r.symbol == row.symbol) {
                            let same_thesis = row.side != "neutral"
                                && row.setup != "waiting"
                                && old.side == row.side
                                && old.setup == row.setup;
                            row.stability_cycles = if same_thesis {
                                old.stability_cycles.saturating_add(1).min(20)
                            } else if row.side != "neutral" && row.setup != "waiting" {
                                1
                            } else {
                                0
                            };
                            row.first_seen_ms = if same_thesis && old.first_seen_ms > 0 {
                                old.first_seen_ms
                            } else {
                                row.checked_ms
                            };
                            row.quality = radar_score(
                                row.base_quality,
                                row.htf_quality,
                                row.execution_quality,
                                row.stability_cycles,
                            );
                            row.phase = if row.blockers.is_empty() && row.stability_cycles >= 2 {
                                "candidate: confirm live flow".into()
                            } else if row.blockers.is_empty() && row.stability_cycles == 1 {
                                "confirming stability".into()
                            } else if row.setup != "waiting" && row.side != "neutral" {
                                "forming".into()
                            } else {
                                "watching".into()
                            };
                            let qualifies = row.phase == "candidate: confirm live flow"
                                && row.blockers.is_empty()
                                && (0.90..=1.0).contains(&row.quality);
                            if qualifies {
                                row.last_qualified_ms = row.checked_ms;
                                row.peak_quality = if same_thesis {
                                    old.peak_quality.max(row.quality)
                                } else {
                                    row.quality
                                };
                            } else if same_thesis {
                                row.last_qualified_ms = old.last_qualified_ms;
                                row.peak_quality = old.peak_quality.max(row.quality);
                            } else {
                                row.last_qualified_ms = 0;
                                row.peak_quality = row.quality;
                            }
                            *old = row;
                        }
                        s.scanned += 1;
                        s.scanning_symbol = format!("{} of {} checked", s.scanned, s.rows.len());
                    }
                }
                {
                    let mut s = store().write();
                    s.status = "monitoring".into();
                    s.scanning_symbol.clear();
                    s.completed_ms = now_ms();
                }
            }
            Err(e) => {
                let mut s = store().write();
                s.status = "unavailable".into();
                s.error = Some(e.to_string());
            }
        }
        tokio::time::sleep(Duration::from_secs(45)).await;
    }
}


#[cfg(test)]
mod radar_tests {
    use super::*;

    #[test]
    fn radar_score_rewards_confirmation_execution_and_stability() {
        let weak = radar_score(0.95, 0.70, 0.40, 1);
        let stable = radar_score(0.95, 0.95, 0.95, 3);
        assert!(stable > weak);
        assert!((0.0..=1.0).contains(&stable));
    }

    #[test]
    fn execution_quality_penalizes_wide_spread_and_chasing() {
        let clean = execution_quality(0.5, 4.0, 0.10);
        let wide = execution_quality(3.9, 4.0, 0.70);
        assert!(clean > wide);
        assert!(execution_quality(f64::NAN, 4.0, 0.1) == 0.0);
    }

    #[test]
    fn perfect_first_scan_does_not_get_full_stability_credit() {
        let first = radar_score(1.0, 1.0, 1.0, 1);
        let mature = radar_score(1.0, 1.0, 1.0, 3);
        assert!(first < mature);
        assert_eq!(mature, 1.0);
    }
}
