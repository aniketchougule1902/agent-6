# Agent-6 Runbook

Copy-paste commands to start the system. Two terminals required.

---

## Prerequisites

| Tool        | Check                | Install                                      |
|-------------|----------------------|----------------------------------------------|
| Rust        | `rustc --version`    | https://rustup.rs                            |
| Node.js 18+ | `node --version`    | https://nodejs.org                           |
| Git         | `git --version`      | https://git-scm.com                          |

---

## Quick Start

### Terminal 1 — Engine

```powershell
cd d:\agent-6
cargo run -p agent6-engine
```

Wait until you see: `Agent-6 local engine started`

### Terminal 2 — Dashboard

```powershell
cd d:\agent-6\apps\ui
npm install
npm run dev
```

### Open Dashboard

Open in browser: **http://127.0.0.1:5173**

Click **ENABLE ALARMS** for browser audio alerts.

---

## Environment Configuration

Copy `.env.example` to `.env` and edit:

```powershell
cd d:\agent-6
copy .env.example .env
```

Key settings in `.env`:

```env
# Market to track (any Bybit linear perpetual)
A6_SYMBOL=BTCUSDT

# Chart timeframe: 1, 3, 5, or 15 (minutes)
A6_TIMEFRAME=3

# Jev qualitative review (optional — obtain from TypeSafe)
TYPESAFE_API_KEY=

# Estimated round-trip trading costs in basis points
A6_ROUND_TRIP_COST_BPS=12

# Signal quality and reward/risk minimums
A6_MIN_SIGNAL_SCORE=0.74
A6_MIN_RR=1.8
```

---

## Paper Trading

The dashboard starts with a **$1,000 paper balance**. When a signal fires:

1. Click **Enter LONG/SHORT** to open a paper position (set your notional)
2. Or enable **Auto-enter** to automatically enter on every new signal
3. The engine monitors TP1, TP2 and stop loss using live exchange prices
4. Click **Close** on any open position to exit at market
5. Click **Reset** to archive the account and start fresh at $1,000

Paper positions persist across restarts in `data/paper-account.json`.

---

## Train & Set Up Calibrated ML Model

To train the champion ML meta-label model, fit probability calibration, evaluate challengers, and deploy the verified model artifact:

```powershell
cd d:\agent-6
python scripts/train_model.py
```

### Options:
- `--symbol`: Market to train on (default: `BTCUSDT`, e.g., `ETHUSDT`, `SOLUSDT`)
- `--limit`: Historical candle count (default: `1000`)
- `--interval`: Kline interval in minutes (default: `1`)
- `--offline`: Use deterministic synthetic market data if offline

### What it does:
1. Fetches historical candles from Bybit Linear REST API.
2. Extracts causal multi-factor features (ADX, RSI, ATR%, MACD, EMAs, VWAP dev, volume ratio, imbalances).
3. Constructs triple-barrier TP-before-SL meta labels.
4. Fits regularized weights with purged chronological splits and embargo.
5. Calibrates raw scores into observed win probabilities using held-out Platt scaling.
6. Evaluates AUC, Brier score, ECE, and compares against LightGBM and CatBoost challengers.
7. Computes optimal utility-based `NO_TRADE` abstention threshold.
8. Exports `models/champion_model.json` with a cryptographic SHA-256 manifest.

When the engine starts, it verifies the model artifact SHA-256 and deploys it. The ServiceBar shows `calibrated model: deployed (champion-v1)` with a green dot, and signals display calibrated win probabilities with abstention filtering.

---

## Run Tests

```powershell
cd d:\agent-6
cargo test --workspace
```

```powershell
cd d:\agent-6\apps\ui
npm run build
```

---

## Change Market

Use the search bar in the dashboard to switch to any Bybit linear perpetual (e.g., ETHUSDT, SOLUSDT, 1000PEPEUSDT, DOGEUSDT).

Or via API:

```powershell
curl -X POST http://127.0.0.1:8787/api/market -H "Content-Type: application/json" -d "{\"symbol\":\"ETHUSDT\",\"timeframe\":\"5\"}"
```

---

## Troubleshooting

| Issue                        | Fix                                                                 |
|------------------------------|---------------------------------------------------------------------|
| Engine won't start           | Check `cargo build -p agent6-engine` for compile errors             |
| UI shows "Connecting..."     | Ensure engine is running on port 8787                               |
| No candles loading           | Wait 30–60s for Bybit REST backfill; check internet connection      |
| Jev not working              | Set `TYPESAFE_API_KEY` in `.env`; restart engine                    |
| Paper account corrupted      | Delete `data/paper-account.json` and restart                        |
| Port in use                  | Change `A6_HTTP_ADDR` in `.env` (default: `127.0.0.1:8787`)        |

---

## Architecture

```
d:\agent-6\
├── apps/engine/     Rust engine — Bybit feed, signals, paper trading, API
├── apps/ui/         React + Vite dashboard
├── data/            Runtime data (journal, paper account, market events)
├── docs/            Architecture, progress, roadmap
├── research/        Python research modules
└── scripts/         Utility scripts
```

Engine serves HTTP API on `:8787`, dashboard proxies through Vite on `:5173`.
