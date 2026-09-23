use crate::{
    state::{now_ms, AppState},
    types::{Candle, TimeframeAnalysis},
};
use serde::Serialize;
use serde_json::Value;
use std::{collections::VecDeque, sync::OnceLock, time::Duration};
#[derive(Clone, Serialize)]
pub struct ScanRow {
    pub symbol: String,
    pub price: f64,
    pub turnover24h: f64,
    pub change24h: f64,
    pub checked_ms: u64,
    pub quality: f64,
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
                        if last <= 0.0 {
                            continue;
                        }
                        rows.push((
                            ScanRow {
                                symbol: symbol.into(),
                                price: last,
                                turnover24h: num("turnover24h"),
                                change24h: num("price24hPcnt") * 100.0,
                                checked_ms: 0,
                                quality: 0.0,
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
                rows.sort_by(|a, b| b.0.turnover24h.total_cmp(&a.0.turnover24h));
                rows.truncate(20);
                {
                    let mut s = store().write();
                    let old = s.rows.clone();
                    s.rows = rows
                        .iter()
                        .map(|(r, _, _)| {
                            if let Some(prev) = old.iter().find(|p| p.symbol == r.symbol) {
                                let mut kept = prev.clone();
                                kept.turnover24h = r.turnover24h;
                                kept.price = r.price;
                                kept.change24h = r.change24h;
                                kept
                            } else {
                                r.clone()
                            }
                        })
                        .collect();
                }
                for (mut row, bid, ask) in rows {
                    store().write().scanning_symbol = row.symbol.clone();
                    let (a, b) = tokio::join!(
                        analysis(&client, state.config.bybit_testnet, &row.symbol, "5"),
                        analysis(&client, state.config.bybit_testnet, &row.symbol, "15")
                    );
                    match (a, b) {
                        (Ok(a), Ok(b)) => {
                            row.quality = a.quality;
                            row.side = a.bias.clone();
                            row.setup = a.setup.clone();
                            row.blockers = a.blockers;
                            if a.bias != b.bias || a.bias == "neutral" {
                                row.blockers.push("15m trend does not confirm".into());
                            }
                            if bid <= 0.0
                                || ask < bid
                                || (ask - bid) / row.price * 10000.0 > state.config.max_spread_bps
                            {
                                row.blockers.push("Spread/liquidity check failed".into());
                            }
                            if (row.price - a.close).abs() > a.atr14 * 0.75 {
                                row.blockers.push("Price moved beyond entry window".into());
                            }
                            if row.quality < state.config.min_signal_score {
                                row.blockers.push("Quality below threshold".into());
                            }
                            if row.side != "neutral" {
                                let d = if row.side == "long" { 1.0 } else { -1.0 };
                                let risk = a.atr14 * 1.5;
                                let cost = row.price * state.config.round_trip_cost_bps / 10000.0;
                                if risk > 0.0
                                    && (risk * 2.5 - cost) / (risk + cost) >= state.config.min_rr
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
                                "candidate: confirm live flow"
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
                    row.checked_ms = now_ms();
                    {
                        let mut s = store().write();
                        if let Some(old) = s.rows.iter_mut().find(|r| r.symbol == row.symbol) {
                            row.first_seen_ms = if old.phase == row.phase
                                && old.side == row.side
                                && old.first_seen_ms > 0
                            {
                                old.first_seen_ms
                            } else {
                                row.checked_ms
                            };
                            *old = row;
                        }
                        s.scanned += 1;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
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
