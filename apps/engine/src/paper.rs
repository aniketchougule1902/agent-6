use crate::{
    state::{now_ms, AppState},
    types::{Side, SignalStatus, TradeSignal},
};
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
const FEE: f64 = 0.00055;
const SLIP: f64 = 0.000035;
#[derive(Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: String,
    pub signal_id: String,
    pub symbol: String,
    pub timeframe: String,
    pub side: Side,
    pub quantity: f64,
    pub remaining: f64,
    pub entry: f64,
    pub stop: f64,
    pub tp1: f64,
    pub tp2: f64,
    pub opened_ms: u64,
    pub expires_ms: u64,
    pub mark: f64,
    pub mark_ms: u64,
    pub tp1_done: bool,
    pub realized_pnl: f64,
    pub fees: f64,
    pub closed_ms: Option<u64>,
    pub exit_reason: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Fill {
    pub position_id: String,
    pub ts_ms: u64,
    pub kind: String,
    pub price: f64,
    pub quantity: f64,
    pub fee: f64,
    pub pnl: f64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub initial_balance: f64,
    pub balance: f64,
    pub positions: Vec<Position>,
    pub fills: Vec<Fill>,
    pub reset_ms: u64,
    pub revision: u64,
}
impl Default for Account {
    fn default() -> Self {
        Self {
            initial_balance: 1000.0,
            balance: 1000.0,
            positions: vec![],
            fills: vec![],
            reset_ms: now_ms(),
            revision: 0,
        }
    }
}
#[derive(Serialize)]
pub struct View {
    pub account: Account,
    pub equity: f64,
    pub available: f64,
    pub reserved: f64,
    pub unrealized: f64,
    pub realized: f64,
    pub fees: f64,
    pub closed_trades: usize,
    pub wins: usize,
    pub win_rate: Option<f64>,
    pub profit_factor: Option<f64>,
    pub max_closed_drawdown: f64,
    pub monitoring: String,
    pub error: Option<String>,
}
pub struct Paper {
    pub account: Account,
    path: PathBuf,
    pub error: Option<String>,
}
fn direction(side: &Side) -> f64 {
    if matches!(side, Side::Long) {
        1.0
    } else {
        -1.0
    }
}
impl Paper {
    pub fn open(path: PathBuf) -> Result<Self> {
        let account = if path.exists() {
            serde_json::from_slice::<Account>(&std::fs::read(&path)?)?
        } else {
            Account::default()
        };
        ensure!(
            account.balance.is_finite() && account.initial_balance == 1000.0,
            "Invalid paper account; preserve file and investigate"
        );
        let this = Self {
            account,
            path,
            error: None,
        };
        this.persist(&this.account)?;
        Ok(this)
    }
    fn persist(&self, account: &Account) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = self.path.with_extension("tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&temp)?;
            f.write_all(&serde_json::to_vec_pretty(account)?)?;
            f.sync_all()?;
        }
        std::fs::rename(temp, &self.path)?;
        Ok(())
    }
    fn commit(&mut self, mut next: Account) -> Result<()> {
        next.revision += 1;
        self.persist(&next)?;
        self.account = next;
        self.error = None;
        Ok(())
    }
    pub fn view(&self) -> View {
        let a = &self.account;
        let open = a.positions.iter().filter(|p| p.remaining > 0.0);
        let reserved = open.clone().map(|p| p.entry * p.remaining).sum::<f64>();
        let unrealized = open
            .clone()
            .map(|p| (p.mark - p.entry) * direction(&p.side) * p.remaining)
            .sum::<f64>();
        let closed = a
            .positions
            .iter()
            .filter(|p| p.closed_ms.is_some())
            .collect::<Vec<_>>();
        let wins = closed.iter().filter(|p| p.realized_pnl > 0.0).count();
        let gains = closed.iter().map(|p| p.realized_pnl.max(0.0)).sum::<f64>();
        let losses = closed
            .iter()
            .map(|p| (-p.realized_pnl).max(0.0))
            .sum::<f64>();
        let mut ordered = closed.clone();
        ordered.sort_by_key(|p| p.closed_ms);
        let mut equity = a.initial_balance;
        let mut peak = equity;
        let mut dd = 0.0_f64;
        for p in ordered {
            equity += p.realized_pnl;
            peak = peak.max(equity);
            dd = dd.max(peak - equity);
        }
        let stale = open
            .clone()
            .any(|p| now_ms().saturating_sub(p.mark_ms) > 10_000);
        View {
            account: a.clone(),
            equity: a.balance + unrealized,
            available: (a.balance - reserved).max(0.0),
            reserved,
            unrealized,
            realized: a.balance - a.initial_balance,
            fees: a.fills.iter().map(|f| f.fee).sum(),
            closed_trades: closed.len(),
            wins,
            win_rate: if closed.is_empty() {
                None
            } else {
                Some(wins as f64 / closed.len() as f64)
            },
            profit_factor: if losses > 0.0 {
                Some(gains / losses)
            } else {
                None
            },
            max_closed_drawdown: dd,
            monitoring: if stale {
                "stale prices; exits paused"
            } else {
                "ready"
            }
            .into(),
            error: self.error.clone(),
        }
    }
    pub fn enter(
        &mut self,
        signal: &TradeSignal,
        notional: f64,
        bid: f64,
        ask: f64,
        now: u64,
    ) -> Result<()> {
        ensure!(
            notional.is_finite() && notional > 0.0,
            "Enter a positive dollar notional"
        );
        ensure!(
            bid.is_finite() && ask.is_finite() && bid > 0.0 && ask >= bid,
            "Valid fresh bid/ask required"
        );
        ensure!(
            signal.status == SignalStatus::Active,
            "Signal already reached a target or ended; wait for a fresh setup"
        );
        ensure!(
            !self
                .account
                .positions
                .iter()
                .any(|p| p.signal_id == signal.id),
            "This signal was already entered in this account"
        );
        let hold = signal
            .timeframe
            .parse::<u64>()?
            .saturating_mul(4)
            .clamp(5, 60)
            * 60_000;
        let expires = signal.created_at_ms + hold;
        ensure!(now < expires, "Signal expired");
        ensure!(now >= signal.created_at_ms && now - signal.created_at_ms <= 60_000,
            "Entry window closed; wait for a fresh setup (60 seconds maximum)");
        ensure!([signal.entry_low, signal.entry_high, signal.stop_loss, signal.tp1, signal.tp2]
            .iter().all(|v| v.is_finite() && *v > 0.0)
            && signal.entry_low <= signal.entry_high, "Invalid setup levels");
        let midpoint = (bid + ask) / 2.0;
        ensure!(midpoint >= signal.entry_low && midpoint <= signal.entry_high,
            "Price is outside the entry zone; do not chase");
        let d = direction(&signal.side);
        let entry = if d > 0.0 {
            ask * (1.0 + SLIP)
        } else {
            bid * (1.0 - SLIP)
        };
        ensure!(
            d * (entry - signal.stop_loss) > 0.0 && d * (signal.tp1 - entry) > 0.0,
            "Market has passed the signal stop or first target; entry rejected"
        );
        ensure!(d * (signal.tp2 - signal.tp1) > 0.0, "Invalid target ordering");
        let quantity = notional / entry;
        let fee = notional * FEE;
        ensure!(
            notional + fee <= self.view().available,
            "Insufficient available paper balance (notional plus entry fee)"
        );
        let mut next = self.account.clone();
        let id = uuid::Uuid::new_v4().to_string();
        next.balance -= fee;
        next.positions.push(Position {
            id: id.clone(),
            signal_id: signal.id.clone(),
            symbol: signal.symbol.clone(),
            timeframe: signal.timeframe.clone(),
            side: signal.side.clone(),
            quantity,
            remaining: quantity,
            entry,
            stop: signal.stop_loss,
            tp1: signal.tp1,
            tp2: signal.tp2,
            opened_ms: now,
            expires_ms: expires,
            mark: (bid + ask) / 2.0,
            mark_ms: now,
            tp1_done: false,
            realized_pnl: -fee,
            fees: fee,
            closed_ms: None,
            exit_reason: None,
        });
        next.fills.push(Fill {
            position_id: id,
            ts_ms: now,
            kind: "entry".into(),
            price: entry,
            quantity,
            fee,
            pnl: -fee,
        });
        self.commit(next)
    }
    fn exit(a: &mut Account, index: usize, qty: f64, price: f64, now: u64, reason: &str) {
        let p = &mut a.positions[index];
        let d = direction(&p.side);
        let execution = price * (1.0 - d * SLIP);
        let fee = execution * qty * FEE;
        let pnl = (execution - p.entry) * d * qty - fee;
        p.remaining = (p.remaining - qty).max(0.0);
        p.realized_pnl += pnl;
        p.fees += fee;
        p.mark = price;
        p.mark_ms = now;
        if p.remaining <= p.quantity * 1e-10 {
            p.remaining = 0.0;
            p.closed_ms = Some(now);
            p.exit_reason = Some(reason.into());
        }
        if reason == "tp1" {
            p.tp1_done = true;
        }
        a.balance += pnl;
        a.fills.push(Fill {
            position_id: p.id.clone(),
            ts_ms: now,
            kind: reason.into(),
            price: execution,
            quantity: qty,
            fee,
            pnl,
        });
    }
    pub fn observe(
        &mut self,
        symbol: &str,
        last: f64,
        bid: f64,
        ask: f64,
        ts: u64,
    ) -> Result<Vec<String>> {
        ensure!(
            [last, bid, ask].iter().all(|v| v.is_finite() && *v > 0.0) && ask >= bid,
            "Invalid quote"
        );
        let mut next = self.account.clone();
        let mut changes = vec![];
        for i in 0..next.positions.len() {
            let p = &mut next.positions[i];
            if p.symbol != symbol || p.remaining == 0.0 || ts < p.mark_ms {
                continue;
            }
            p.mark = last;
            p.mark_ms = ts;
            let d = direction(&p.side);
            let exit = if d > 0.0 { bid } else { ask };
            let reason = if d * (last - p.stop) <= 0.0 {
                Some("stop_loss")
            } else if ts >= p.expires_ms {
                Some("expired")
            } else {
                None
            };
            if let Some(reason) = reason {
                let qty = p.remaining;
                Self::exit(&mut next, i, qty, exit, ts, reason);
                changes.push(reason.into());
                continue;
            }
            if !p.tp1_done && d * (last - p.tp1) >= 0.0 {
                let qty = p.remaining / 2.0;
                Self::exit(&mut next, i, qty, exit, ts, "tp1");
                changes.push("tp1".into());
            }
            let p = &next.positions[i];
            if d * (last - p.tp2) >= 0.0 {
                let qty = p.remaining;
                Self::exit(&mut next, i, qty, exit, ts, "tp2");
                changes.push("tp2".into());
            }
        }
        if changes.is_empty() {
            self.account = next;
            self.error = None;
        } else {
            self.commit(next)?;
        }
        Ok(changes)
    }
    pub fn close(&mut self, id: &str, bid: f64, ask: f64, ts: u64) -> Result<()> {
        ensure!(
            [bid, ask].iter().all(|p| p.is_finite() && *p > 0.0) && ask >= bid,
            "Invalid quote"
        );
        let i = self
            .account
            .positions
            .iter()
            .position(|p| p.id == id && p.remaining > 0.0)
            .ok_or_else(|| anyhow::anyhow!("Open position not found"))?;
        let p = &self.account.positions[i];
        ensure!(ts >= p.opened_ms, "Quote predates entry");
        let price = if direction(&p.side) > 0.0 { bid } else { ask };
        let qty = p.remaining;
        let mut next = self.account.clone();
        Self::exit(&mut next, i, qty, price, ts, "manual");
        self.commit(next)
    }
    pub fn reset(&mut self) -> Result<()> {
        let archive = self.path.with_file_name(format!(
            "paper-account-archive-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::copy(&self.path, archive)?;
        let next = Account {
            revision: self.account.revision,
            ..Account::default()
        };
        self.commit(next)
    }
}
#[derive(Clone, Copy)]
pub struct Quote {
    pub last: f64,
    pub bid: f64,
    pub ask: f64,
    pub ts: u64,
}
pub async fn quotes(testnet: bool) -> Result<std::collections::HashMap<String, Quote>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let hosts = if testnet {
        vec!["https://api-testnet.bybit.com"]
    } else {
        vec!["https://api.bytick.com", "https://api.bybit.com"]
    };
    for host in hosts {
        let Ok(r) = client
            .get(format!("{host}/v5/market/tickers?category=linear"))
            .send()
            .await
        else {
            continue;
        };
        let Ok(v) = r.json::<serde_json::Value>().await else {
            continue;
        };
        if v["retCode"] != 0 {
            continue;
        }
        let ts = v["time"].as_u64().unwrap_or(0);
        if now_ms().abs_diff(ts) > 10_000 {
            continue;
        }
        let Some(rows) = v["result"]["list"].as_array() else {
            continue;
        };
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let number = |k: &str| {
                row[k]
                    .as_str()
                    .and_then(|x| x.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            if let Some(symbol) = row["symbol"].as_str() {
                let q = Quote {
                    last: number("lastPrice"),
                    bid: number("bid1Price"),
                    ask: number("ask1Price"),
                    ts,
                };
                if [q.last, q.bid, q.ask]
                    .iter()
                    .all(|x| x.is_finite() && *x > 0.0)
                    && q.ask >= q.bid
                {
                    out.insert(symbol.into(), q);
                }
            }
        }
        return Ok(out);
    }
    bail!("Fresh exchange quotes unavailable")
}
pub async fn run(state: AppState) {
    loop {
        let symbols = state
            .paper
            .lock()
            .account
            .positions
            .iter()
            .filter(|p| p.remaining > 0.0)
            .map(|p| p.symbol.clone())
            .collect::<std::collections::HashSet<_>>();
        if !symbols.is_empty() {
            match quotes(state.config.bybit_testnet).await {
                Ok(quotes) => {
                    for symbol in symbols {
                        if let Some(q) = quotes.get(&symbol) {
                            let result = state
                                .paper
                                .lock()
                                .observe(&symbol, q.last, q.bid, q.ask, q.ts);
                            match result {
                                Ok(events) => {
                                    for kind in events {
                                        let alert = match kind.as_str() {
                                            "tp1" => crate::types::AlertKind::Tp1,
                                            "tp2" => crate::types::AlertKind::Tp2,
                                            "stop_loss" => crate::types::AlertKind::StopLoss,
                                            _ => crate::types::AlertKind::Expired,
                                        };
                                        state.publish(crate::types::EngineEvent{ts_ms:q.ts,event_type:format!("paper_{kind}"),alert:Some(alert),message:format!("Paper account {symbol}: {kind}; see account ledger"),signal:None});
                                    }
                                }
                                Err(e) => state.paper.lock().error = Some(e.to_string()),
                            }
                        }
                    }
                }
                Err(e) => state.paper.lock().error = Some(e.to_string()),
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(now: u64, side: Side) -> TradeSignal {
        let d = direction(&side);
        TradeSignal {
            id: "test".into(),
            symbol: "BTCUSDT".into(),
            timeframe: "5".into(),
            side,
            status: SignalStatus::Active,
            created_at_ms: now,
            entry_low: 100.0,
            entry_high: 100.0,
            stop_loss: 100.0 - d * 2.0,
            tp1: 100.0 + d * 3.0,
            tp2: 100.0 + d * 5.0,
            risk_reward_tp2: 2.5,
            confidence: 0.8,
            calibrated: false,
            invalidation: String::new(),
            reasons: vec![],
            last_event_ms: now,
            observed_exit_price: None,
        }
    }
    fn account() -> Paper {
        Paper::open(
            std::env::temp_dir().join(format!("a6-paper-test-{}.json", uuid::Uuid::new_v4())),
        )
        .unwrap()
    }
    #[test]
    fn rejects_late_chased_future_and_invalid_entries_without_mutation() {
        let mut p = account();
        let now = now_ms();
        for side in [Side::Long, Side::Short] {
            let mut s = fixture(now, side);
            assert!(p.enter(&s, 100.0, 99.99, 100.01, now + 60_001).is_err());
            assert!(p.enter(&s, 100.0, 99.99, 100.01, now - 1).is_err());
            assert!(p.enter(&s, 100.0, 100.99, 101.01, now).is_err());
            s.tp2 = s.stop_loss;
            assert!(p.enter(&s, 100.0, 99.99, 100.01, now).is_err());
            s.entry_low = f64::NAN;
            assert!(p.enter(&s, 100.0, 99.99, 100.01, now).is_err());
        }
        assert_eq!(p.account.balance, 1000.0);
        assert!(p.account.positions.is_empty());
        assert!(p.account.fills.is_empty());
        std::fs::remove_file(p.path).unwrap();
    }
    #[test]
    fn account_roundtrip_partial_exit_and_reload() {
        let mut p = account();
        let now = now_ms();
        let s = fixture(now, Side::Long);
        p.enter(&s, 100.0, 99.99, 100.01, now).unwrap();
        assert!(p.view().available < 900.0);
        assert!(p.enter(&s, 100.0, 99.99, 100.01, now).is_err());
        let original = p.account.positions[0].quantity;
        p.observe("BTCUSDT", 103.0, 102.99, 103.01, now + 100)
            .unwrap();
        assert!((p.account.positions[0].remaining - original / 2.0).abs() < 1e-9);
        p.observe("BTCUSDT", 106.0, 105.99, 106.01, now + 200)
            .unwrap();
        let reloaded = Paper::open(p.path.clone()).unwrap();
        assert_eq!(reloaded.view().closed_trades, 1);
        assert!(reloaded.account.balance > 1000.0);
        assert_eq!(reloaded.account.fills.len(), 3);
        let before = reloaded.account.balance;
        p.observe("BTCUSDT", 107.0, 106.99, 107.01, now + 300)
            .unwrap();
        assert!((p.account.balance - before).abs() < 1e-9);
        std::fs::remove_file(p.path).unwrap();
    }
    #[test]
    fn short_stop_and_insufficient_funds() {
        let mut p = account();
        let now = now_ms();
        let s = fixture(now, Side::Short);
        assert!(p.enter(&s, 1000.0, 99.99, 100.01, now).is_err());
        assert!(p.enter(&s, f64::NAN, 99.99, 100.01, now).is_err());
        p.enter(&s, 100.0, 99.99, 100.01, now).unwrap();
        p.observe("BTCUSDT", 104.0, 103.99, 104.01, now + 1)
            .unwrap();
        assert!(p.account.balance < 1000.0);
        assert_eq!(
            p.account.positions[0].exit_reason.as_deref(),
            Some("stop_loss")
        );
        std::fs::remove_file(p.path).unwrap();
    }
    #[test]
    fn failed_persistence_does_not_create_position() {
        let mut p = account();
        let old = p.path.clone();
        p.path = old.with_extension("directory");
        std::fs::create_dir(&p.path).unwrap();
        assert!(p
            .enter(
                &fixture(now_ms(), Side::Long),
                100.0,
                99.99,
                100.01,
                now_ms()
            )
            .is_err());
        assert!(p.account.positions.is_empty());
        std::fs::remove_file(old).unwrap();
        let _ = std::fs::remove_file(p.path.with_extension("tmp"));
        std::fs::remove_dir(p.path).unwrap();
    }
}
