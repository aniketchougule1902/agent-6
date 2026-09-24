> Current operation: see [runbook.md](RUNBOOK.md) for the one-command launcher and current limits. The demo has been removed. The local paper account starts at $1,000. Market Radar scans the top 200 USDT crypto perpetual markets by turnover and lists fresh 90-100% setup-quality candidates in descending score order; rankings are not win probabilities. Legacy calibration artifacts are blocked until independent live-outcome validation passes. Historical checkpoints below describe earlier versions.

## Start the current app

Requires Rust stable, Node.js 22.12+ or 24+, and Python 3.12+. In PowerShell:

```powershell
cd D:\agent-6
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start.ps1
```

Open http://127.0.0.1:5173. The launcher builds the release engine and UI, preserves existing configuration, starts both services hidden and checks HTTP readiness. See the runbook for stop, restart, dependency installation and recovery commands.

### Current features and limits

- Persistent $1,000 paper account: manual signal entry, optional browser-session auto-entry, partial TP1, TP2/SL/expiry/manual exits, reset confirmation and archived account history. Signal IDs are deterministic from entry evidence; confirmed opposite setups explicitly mark the prior setup `reversed`, and recent terminal/invalidation history remains visible.
- Balance, equity, available/reserved funds, fees, realized/unrealized P&L, fills, win rate, profit factor and closed-equity drawdown. Paper positions survive restarts and market changes; fresh quote polling monitors them independently.
- Background radar scans the 200 highest-turnover USDT crypto perpetuals, including eligible meme coins, using 5m formations and 15m confirmation. The radar lists only fresh, unblocked 90-100% quality candidates, sorted by score, with provisional levels and observation age. Selecting a row opens that market for full live flow confirmation.
- Agent activity exposes scan progress, Jev's latest actual response/error and calibration progress. Jev is online only after a successful typed response.
- Live calibration collects versioned, observed outcomes and runs every five minutes. At least 500 outcomes and independent validation gates are required before deployment. Until then quality scores remain heuristic; 90% accuracy is not established.
- Single-user local paper terminal. No real-money execution. Remote production hosting still requires authentication, TLS, durable multi-user storage and operational testing. Quote polling can miss price crossings between observations.

# Agent-6 — Local Crypto Scalping Research & Signal Engine

Agent-6 is a **local-first, continuously running crypto market analysis workstation**. It watches live exchange data, builds multi-timeframe and microstructure features, emits typed trade setups, tracks TP/SL lifecycle events, rings audible alerts, journals every decision, and provides a gated path for self-improving models.

> **Status:** active build. The default mode is analysis/paper signals. No real-money order execution is enabled in the initial milestone.

## Core principles

- **Local only:** no cloud hosting is required.
- **Always-on while the process is running:** reconnecting market streams, heartbeat monitoring, stale-feed protection, health checks, and alarms.
- **Fast typed path:** Rust owns ingestion, exact local L50 book state, market features, signal lifecycle, risk checks, and event delivery.
- **Type-safe UI contract:** Rust types are exported to TypeScript with `ts-rs`; TypeScript runs with strict mode.
- **AI is not in the critical latency path:** ML/LLM research can suggest challengers, but live decisions stay deterministic and bounded.
- **Self-evolution is gated:** challengers must pass replay, walk-forward, cost/slippage stress, shadow evaluation, and promotion rules before they can replace the champion.
- **Abstention first:** the system is allowed to return `NO_TRADE`. High trade frequency is not a goal.
- **Persistent memory:** every signal, feature snapshot, lifecycle event, and later model outcome is journaled locally.

## Initial architecture

```text
Bybit V5 public WS/REST
        |
        v
Rust Market Gateway
  | trades / exact L50 snapshot+delta book / ticker / liquidations
  | 1m / 3m / 5m / 15m candles
        |
        v
Typed Market State
        |
        +--> Feed Integrity Gate
        |      event age / order-book age / reconnect state
        |
        +--> Feature Engine
        |      HTF trend / ATR / VWAP / momentum
        |      spread / L50 + top5 imbalance / microprice
        |      trade flow / OI / funding / liquidation / regime
        |
        +--> Signal + Risk Engine
        |      LONG / SHORT / NO_TRADE
        |      entry / SL / TP1 / TP2 / invalidation
        |
        +--> Lifecycle Monitor
        |      signal -> TP1 -> TP2 or SL / expired
        |
        +--> Journal
        |
        +--> WebSocket/API
                  |
                  v
          Local TypeScript UI
          chart + overlays + alarms
```

## Audible events

The local dashboard rings distinct tones for:

- new qualified signal
- TP1 hit
- TP2 hit
- stop-loss hit
- invalidation/expiry
- market-feed disconnect/reconnect
- stale-feed halt/recovery
- future model promotion/drift alerts

The Rust process also emits a terminal bell as a secondary fallback for critical events.

> Browsers require a user gesture before WebAudio is allowed. Click **ENABLE ALARMS** once after opening the dashboard; after that, signal/TP/SL/feed events ring automatically while the dashboard is open.

## Requirements

Install:

- Rust stable: https://rustup.rs
- Node.js 22+ and npm
- Python 3.12+ for research tests and hourly checks
- Git

No API key is required for the first public-market-data milestone.

## Local setup

### Windows PowerShell

```powershell
git clone https://github.com/aniketchougule1902/agent-6.git
cd agent-6

Copy-Item .env.example .env

# Terminal 1: Rust engine
cargo run -p agent6-engine

# Terminal 2: dashboard
cd apps/ui
npm install
npm run dev
```

Open:

```text
http://127.0.0.1:5173
```

### Linux/macOS

```bash
git clone https://github.com/aniketchougule1902/agent-6.git
cd agent-6

cp .env.example .env

# Terminal 1
cargo run -p agent6-engine

# Terminal 2
cd apps/ui
npm install
npm run dev
```

Open `http://127.0.0.1:5173`.

## Configuration

Environment variables:

```dotenv
A6_SYMBOL=BTCUSDT
A6_BYBIT_TESTNET=false
A6_HTTP_ADDR=127.0.0.1:8787
A6_TIMEFRAME=3
A6_MIN_SIGNAL_SCORE=0.74
A6_MIN_RR=1.8
A6_SIGNAL_COOLDOWN_SECS=90
A6_MAX_SPREAD_BPS=4.0
A6_STALE_FEED_MS=3500
A6_JOURNAL_PATH=data/journal.jsonl
A6_MARKET_RECORD_PATH=data/market-events.jsonl
# Optional:
TYPESAFE_API_KEY=
```

Use `BTCUSDT`, `ETHUSDT`, or another Bybit linear perpetual symbol supported by the public feed.

### Change market without restarting

With the local engine and dashboard running, use the symbol field and 1m/3m/5m/15m buttons in the dashboard. Symbol changes explicitly invalidate any still-open setup, retain it in signal history, clear old symbol-specific market state, backfill the new market and rebuild the Bybit subscription without restarting the process.

The same control is available through the local API:

```bash
curl -X POST http://127.0.0.1:8787/api/market \
  -H "content-type: application/json" \
  -d '{"symbol":"ETHUSDT","timeframe":"5"}'

curl http://127.0.0.1:8787/api/market
```

`A6_TIMEFRAME` selects the setup label/execution timeframe. The chart and engine can switch among 1m/3m/5m/15m at runtime without restarting.

The dashboard uses TradingView **Lightweight Charts** with Bybit market data. Click **Open in TradingView** to inspect the selected symbol in a separate TradingView tab. Entry, stop and target lines are drawn in this dashboard; **Download setup drawing** saves those lines with the chart as a PNG. They are not written into a TradingView account. TradingView's terms prohibit automated collection of its site data, so Agent-6 obtains history and live prices directly from the exchange.

### Optional TypeSafe Jev review

Set `TYPESAFE_API_KEY` in `.env` to enable one asynchronous Jev confluence review per emitted setup. The review appears in the event journal. It receives only bounded market features, never credentials or account data. Failure or latency leaves the Rust signal and TP/SL monitoring running. Jev's model certainty is not the probability that the trade wins, and its review does not promote or block a setup. The feature needs a TypeSafe key and has not been validated as an accuracy improvement.

## Development commands

From the repository root:

```bash
# Compile/check the Rust engine
cargo check --workspace

# Run Rust tests and regenerate TypeScript bindings
cargo test --workspace

# Engine with logs
RUST_LOG=agent6_engine=debug cargo run -p agent6-engine

# UI
cd apps/ui
npm install
npm run dev

# UI typecheck
npm run typecheck

# UI production build
npm run build

# Research tests (from repository root)
cd ../..
python -m pip install -e ./research pytest
cd research
python -m pytest -q tests

# Build typed DuckDB + Parquet storage from the normalized recorder
cd ..
python scripts/build_event_store.py --overwrite
```

### Hourly 24-hour verification

In another terminal from the repo root, run `python scripts/hourly-check.py`. It runs Rust, research and UI checks once per hour for 24 hours, records pass/fail details in `data/hourly-check.jsonl`, and exits. Keep the machine and this terminal running. This is a check schedule; the remaining roadmap items still require implementation and review. The script does not trade or change source code.

PowerShell log example:

```powershell
$env:RUST_LOG="agent6_engine=debug"
cargo run -p agent6-engine
```

## Safety / validation

A displayed `confidence` in the first heuristic milestone is a **setup-quality score, not a calibrated probability of profit**. The project roadmap replaces this with out-of-sample calibrated probabilities after sufficient replay/training data exists.

The engine refuses new setups when the feed or order-book data exceeds its freshness threshold.

The engine must account for fees, spread, slippage, latency, adverse selection, and drawdown before any strategy is considered promotable. A 90% win-rate target is **not** treated as a success criterion; positive after-cost expectancy and controlled tail risk are.

Real-money autonomous execution stays disabled until the repository contains explicit exchange-auth, execution-simulation, paper/shadow evidence, kill switches, and risk controls.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [24-hour build plan](docs/ROADMAP_24H.md)
- [Progress log](docs/PROGRESS.md)
- [Validation rules](docs/VALIDATION.md)

## Data sources

The first gateway targets Bybit V5 public linear-perpetual endpoints:

- live klines: 1m/3m/5m/15m
- public trades
- L50 snapshot/delta order book
- derivatives ticker
- all-liquidation stream
- REST historical kline backfill

The implementation uses documented heartbeat/reconnect behavior and keeps exchange-specific code behind an adapter so Binance/OKX/Coinbase/Hyperliquid can be added later.

Historical backfill retries every 15 seconds when Bybit REST is unavailable and can use Bybit's documented alternate mainnet host `api.bytick.com`. The health endpoint reports `ok=false` until live data and enough history produce features; the dashboard shows **WARMING** during this period.

Accepted WebSocket payloads are normalized once, validated, atomically recorded to `A6_MARKET_RECORD_PATH`, and the exact same typed events mutate live Rust market state. `python scripts/build_event_store.py --overwrite` converts that JSONL into typed Zstd Parquet plus DuckDB for offline research. Strict replay now requires exact kline `start_ms`/`end_ms`; recordings created before that schema addition should be treated as legacy and re-recorded or explicitly migrated rather than silently replayed. REST bootstrap candles are still a documented replay-parity gap, so full startup-equivalent feature/signal replay is not yet claimed complete.

## License

Private research/educational code unless and until a license is added.


## Advanced scalping dashboard update (2026-09-23)

- Independent paper setups and target/stop alerts for all supported scalping intervals: 1m, 3m, 5m and 15m. Changing the displayed timeframe preserves the other timeframe setups. Changing the symbol or restarting resets active tracking; journal chart flags remain available.
- Chart entry arrows and TP1-touch, TP2-exit and stop-exit flags use observed public-trade prices. These are paper observations, not exchange fills. Existing journals without observed exit prices cannot reconstruct exit flags. Journal-tail reload retains up to 1,000 flags from the last 2 MB.
- Toggle EMA 9/21/50, rolling VWAP20, Bollinger20/2, volume and flags from other intervals. Download the current setup drawing when a setup exists.
- Closed-candle analysis includes Wilder RSI14, ADX14, ATR14, MACD histogram, relative volume and prior-20-bar support/resistance. Trend pullbacks and channel breakouts require momentum, participation, directional agreement and live flow checks. The board explains blocked setups.
- Admission checks reject stale/gapped data, excessive spread, price chasing, abnormal volatility, unfavorable cost-adjusted reward/risk and repeated entries on the same candle. `A6_ROUND_TRIP_COST_BPS=12` estimates round-trip fees/slippage before live spread; set it to realistic venue/order costs. It does not simulate fills.
- Jev remains an optional asynchronous review of the selected setup. Scores are uncalibrated quality, not win probabilities. More filters have not demonstrated a higher win rate; 90%+ accuracy is neither established nor guaranteed.
- Local run: `cargo run -p agent6-engine`; in another terminal `cd apps/ui` then `npm run dev -- --host 127.0.0.1`. Open http://127.0.0.1:5173 and click Enable Alarms for browser audio. The server and browser must remain running for browser alarms.
- Verified: 60 Rust tests pass, UI production build passes, live API exposes four analyses and 500 1m history candles, and browser loads with no page errors. Tests cover independent long/short lifecycles, price gaps across both targets, duplicate touches, old ticks and expiry. No live orders are submitted. Research training and out-of-sample profitability remain separate validation work.


## Chart terminal and market catalog (2026-09-23)

The chart-focused layout takes visual direction from the supplied screenshot: directional candle colors, an EMA21/50 trend ribbon, paper entry/exit flags, and translucent reward/risk rectangles with entry, stop and TP2 labels. This is an original implementation, not LuxAlgo proprietary logic or a claim of superior performance. The cloud is a trend visualization, not an additional independent prediction. Actual exchange candles remain the price source; synthetic Heikin-Ashi prices are not used for outcomes.

### Market selection

Search by coin or ticker using the top combobox (arrow keys, Enter, Escape supported). `/api/instruments` paginates Bybit linear instruments, filters active perpetual contracts, caches the catalog for 15 minutes, and validates selections before switching. The live test returned 845 markets, including DOGE, PEPE and BONK contracts. Coverage is all currently listed Bybit linear perpetuals, including USDT and USDC; it does not include every token on every exchange or DEX. Coin icons use cryptocurrency-icons and ErikThiart/cryptocurrency-icons, with initials when no image exists. They are display assets, not token identity verification.

### Chart, sizing and status

- Toggle ribbon, trend candle colors, EMA, VWAP, Bollinger bands and TP/SL shading. Export PNG includes the custom ribbon and risk rectangles; export also works while waiting for a signal.
- TP/SL levels round outward to the exchange tick size and recheck net estimated reward/risk. Admission waits for instrument metadata. Companion timeframe confirmation maps 1m to 3m, 3m to 5m, 5m to 15m, and 15m to 5m.
- Position planner uses account value in quote currency, risk percentage, estimated round-trip costs and venue quantity increments. It is informational, submits no orders, and excludes funding, gaps, liquidation and margin constraints.
- Top service bar polls actual local API health every four seconds. It distinguishes feed live/unavailable, history ready/loading, signal evaluating/halted, Jev configured/not configured, paper execution, calibrated model not deployed, dashboard connectivity and browser audio. Jev configuration does not prove remote service availability.
- Entry/exit flags are paper observations. Active positions are not restored across process restarts or symbol changes. The persistent journal preserves available flag history.

### Verification

62 Rust tests pass, including market filtering and tick precision for tiny meme prices and large BTC prices. Production TypeScript/Vite build passed. Browser checks verified service status, search, live BTC-to-1000PEPEUSDT switch, meme logo, 500 historical candles and no page errors. An isolated browser fixture verified the risk-zone renderer without inserting a signal into the live system. Current quality scores remain uncalibrated. Comparative win rate against LuxAlgo, profitability and 90% accuracy have not been established.

Data contract: https://bybit-exchange.github.io/docs/v5/market/instrument
Chart API: https://tradingview.github.io/lightweight-charts/docs/api/interfaces/IChartApi

## Cross-timeframe visibility fix

An empty selected interval previously hid open setups from other intervals. The dashboard now selects an open paper setup automatically, provides a setup selector independent of chart interval, and displays its levels, shaded zones, flags and sizing on the selected chart. Labels state both timeframes. New-entry blockers are distinct from existing trade monitoring; completed setups are labeled last setup. Chart price scaling includes all displayed setup levels. Default flags follow the displayed setup; All timeframe flags expands the history. Verified on the live 1m chart with the 5m short and five setup-selection regression checks. No trading thresholds changed.
