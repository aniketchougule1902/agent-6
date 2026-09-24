# Agent-6 local paper terminal runbook

Single-user local paper trading. No real-money orders. This is not a certified unattended production trading platform.

The dashboard now opens in Beginner mode with one next-step card. Advanced view reveals market radar and detailed analysis. New paper entries require a setup no older than 60 seconds and a current price inside its entry zone; the server enforces these checks too. A silent dashboard halts entry after five seconds. Enable alarms after opening the page. See [production readiness](docs/PRODUCTION_READINESS.md) for the audit and prioritized next work.

## One-command start (PowerShell)

Prerequisites: Rust stable, Node.js 22.12+ or 24+, Python 3.12+.

```powershell
cd D:\agent-6
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start.ps1
```

Builds the release engine and UI, starts services hidden, and checks HTTP readiness. First build can take several minutes. Existing `.env` is preserved. Open http://127.0.0.1:5173 and enable browser alarms.

Already built:

```powershell
cd D:\agent-6
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start.ps1 -SkipBuild
```

Stop launcher-owned services:

```powershell
cd D:\agent-6
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\stop.ps1
```

For upgrades, stop first and start without SkipBuild. Manually launched terminals must be stopped with Ctrl+C. Logs are under `data/logs/`.

## Research and calibration

```powershell
cd D:\agent-6
python -m pip install -e .\research pytest
python -m pytest -q .\research\tests
python .\scripts\calibrate_signals.py
```

The engine runs calibration every five minutes. Only real versioned signal outcomes qualify. Deployment requires at least 500 completed outcomes, both classes, purged chronological train/calibration/test partitions, independent test Brier below baseline, and ECE <= 0.08. Until then the app reports collecting/rejected, not invented accuracy. The model estimates TP2-before-stop/expiry, not net account profitability. Legacy `scripts/train_model.py` and candle-proxy artifacts are research only and cannot pass this live deployment gate.

Set TYPESAFE_API_KEY in `.env` and restart for Jev. Green online means a recent valid response; last review and errors appear in Agent activity. Reviews cover new setups and periodic market context. Jev never authorizes orders or supplies calibrated trade accuracy.

## Paper account

- Starts at $1,000. Enter a dollar notional from an active signal. Entry uses current bid/ask plus simulated slippage, never the old signal price.
- At 1x, notional is reserved; fees are 5.5 bps and simulated slippage 0.35 bps each way. TP1 closes half; TP2, SL, expiry or manual Close exits the rest.
- Fractional simulated quantities are supported. Funding, liquidation, queue position and exchange order constraints are not modeled.
- Positions survive symbol changes and restarts. Fresh quote polling runs every two seconds for open symbols. During outages exits pause; crossings between observations may be missed.
- Account shows balance, equity, reserved/available funds, realized/unrealized P&L, fees, fills, win rate, profit factor and closed-equity drawdown.
- Reset requires confirmation, archives the old account, cancels paper positions and restores $1,000.
- Auto-enter is off after reload. It is a browser-session option and requires the tab to remain open. Scanner candidates never auto-enter.

## Market radar

Radar scans up to 200 active USDT crypto perpetuals selected by 24h quote turnover; stock, ETF, commodity and forex contracts are excluded. Four market scans run concurrently, each checking 5m setups with 15m confirmation. After a cycle, the scanner waits 45 seconds before starting again. Duration depends on exchange latency and errors.

The active list shows only fresh candidates with no scan blockers and composite radar quality from 90% through 100%, sorted by descending score. The score combines 5m setup structure (50%), 15m confirmation quality (25%), current execution quality from spread/entry displacement (15%), and multi-scan stability (10%). Turnover selects the broad 200-market liquidity universe but never determines displayed rank. A setup must survive at least two consecutive scans with the same side/setup thesis before it can become active. Recently qualified same-thesis setups that temporarily lose a gate remain visible for up to four minutes under Revalidating instead of silently disappearing; they are not treated as active until the gates recover. Setup quality remains a heuristic score, not a calibrated win probability. Empty active results mean no qualifying setups; scores are never inflated to populate the list. Rows older than 180 seconds are excluded, with age measured from the ticker snapshot used for price/spread checks. Selecting a coin opens full live order-flow confirmation; radar candidates do not submit orders.

The chart supports 1m/3m/5m/15m. Only the selected market receives the full live order-book/trade-flow signal pipeline. Existing paper positions have independent quote monitoring.

Signal admission v3 is deliberately stricter than the radar score. Breakouts must close with sufficient ATR-normalized channel clearance, a strong candle body, a close near the directional edge of the candle, and no exhaustion-sized range. After the candle closes, the live price must continue to hold the breakout/pullback structure while 1m momentum, companion-timeframe trend/momentum, spread, and an eight-component microstructure ensemble remain aligned. All hard gates must stay valid continuously for `A6_SIGNAL_CONFIRM_MS` (default 6000 ms) before a paper signal is admitted; any hard-gate failure resets the arming timer. `A6_MIN_LIVE_EDGE` controls the required directional microstructure edge. This is designed to reject immediate post-breakout fakeouts; it does not make losses impossible.

The v3 quality formula is continuous rather than awarding near-full credit for binary MACD/setup checks. v2 calibrated artifacts are intentionally rejected because the score distribution and admission policy changed. Calibration restarts using only completed `strategy:a6-live-v3` outcomes, so a percentage shown as raw quality remains a setup/admission score until a new independently validated v3 calibration artifact exists.

## Data, security and recovery

Paper state: `data/paper-account.json`; reset archives: `data/paper-account-archive-*.json`. Back up while stopped. Persistence failure rejects mutations. Corrupt account JSON prevents startup instead of silently resetting money.

Live journal and market recordings are preserved for audit/calibration. Generated screenshots are archived under `data/archive`. Scripted demo endpoints and UI are removed.

The API binds only to loopback and rejects unapproved browser origins. Remote/multi-user deployment needs authentication, TLS, durable storage, monitoring and further load/failure testing.

## Manual development

Terminal 1:

```powershell
cd D:\agent-6
cargo run -p agent6-engine
```

Terminal 2:

```powershell
cd D:\agent-6\apps\ui
npm ci
npm run dev -- --host 127.0.0.1
```

Validation:

```powershell
cd D:\agent-6
cargo test --workspace
npm run build --prefix apps/ui
python -m pytest -q research/tests
```
