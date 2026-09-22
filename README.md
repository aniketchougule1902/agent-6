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
```

Use `BTCUSDT`, `ETHUSDT`, or another Bybit linear perpetual symbol supported by the public feed.

### Change market without restarting

With the local engine and dashboard running, use the symbol field and 1m/3m/5m/15m buttons in the dashboard. Symbol changes clear old symbol-specific state, backfill the new market and rebuild the Bybit subscription without restarting the process.

The same control is available through the local API:

```bash
curl -X POST http://127.0.0.1:8787/api/market \
  -H "content-type: application/json" \
  -d '{"symbol":"ETHUSDT","timeframe":"5"}'

curl http://127.0.0.1:8787/api/market
```

`A6_TIMEFRAME` is the setup label/execution timeframe for the current engine milestone; the chart itself can switch among 1m/3m/5m/15m. Runtime symbol/timeframe switching without restart is on the 24-hour roadmap.

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
```

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

## License

Private research/educational code unless and until a license is added.
