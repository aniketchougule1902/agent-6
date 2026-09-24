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

The list shows only fresh candidates with no scan blockers and raw setup quality from 90% through 100%, sorted by descending quality. No volume-ranking tab remains. The top-200 universe uses turnover, not global market capitalization. Setup quality is a heuristic score, not a calibrated win probability. Empty results mean no qualifying setups; scores are never inflated to populate the list. Rows older than 180 seconds are excluded, with age measured from the ticker snapshot used for price/spread checks. Selecting a coin opens full live order-flow confirmation; radar candidates do not submit orders.

The chart supports 1m/3m/5m/15m. Only the selected market receives the full live order-book/trade-flow signal pipeline. Existing paper positions have independent quote monitoring.

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
