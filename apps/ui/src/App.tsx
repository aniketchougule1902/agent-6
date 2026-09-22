import { useEffect, useMemo, useRef, useState } from "react";
import {
  CandlestickSeries,
  ColorType,
  createChart,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type UTCTimestamp,
} from "lightweight-charts";
import type { AlertKind } from "./generated/AlertKind";
import type { EngineEvent } from "./generated/EngineEvent";
import type { EngineSnapshot } from "./generated/EngineSnapshot";
import type { TradeSignal } from "./generated/TradeSignal";

type Timeframe = "1" | "3" | "5" | "15";
type WsEnvelope =
  | { kind: "snapshot"; data: EngineSnapshot }
  | { kind: "event"; data: EngineEvent };

const WS_URL = import.meta.env.VITE_ENGINE_WS ?? "ws://127.0.0.1:8787/ws";

function useAudioAlarms() {
  const context = useRef<AudioContext | null>(null);
  const [enabled, setEnabled] = useState(false);

  async function enable() {
    const audio = context.current ?? new AudioContext();
    context.current = audio;
    await audio.resume();
    setEnabled(true);
    play("feed_reconnected");
  }

  function play(kind: AlertKind) {
    const audio = context.current;
    if (!enabled || !audio) return;

    const patterns: Record<AlertKind, [number, number, number]> = {
      signal: [880, 0.16, 3],
      tp1: [1046, 0.12, 2],
      tp2: [1318, 0.18, 4],
      stop_loss: [220, 0.28, 3],
      expired: [330, 0.15, 2],
      feed_disconnected: [180, 0.35, 2],
      feed_reconnected: [660, 0.10, 1],
      drift: [260, 0.22, 4],
      model_promoted: [1174, 0.13, 4],
    };

    const [frequency, duration, count] = patterns[kind];
    for (let i = 0; i < count; i += 1) {
      const start = audio.currentTime + i * (duration + 0.07);
      const oscillator = audio.createOscillator();
      const gain = audio.createGain();
      oscillator.type = kind === "stop_loss" ? "sawtooth" : "sine";
      oscillator.frequency.setValueAtTime(frequency, start);
      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.exponentialRampToValueAtTime(0.22, start + 0.015);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
      oscillator.connect(gain);
      gain.connect(audio.destination);
      oscillator.start(start);
      oscillator.stop(start + duration + 0.02);
    }
  }

  return { enabled, enable, play };
}

function Chart({
  snapshot,
  timeframe,
}: {
  snapshot: EngineSnapshot;
  timeframe: Timeframe;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const signalLines = useRef<IPriceLine[]>([]);

  const candles = useMemo(() => {
    if (timeframe === "1") return snapshot.candles_1m;
    if (timeframe === "3") return snapshot.candles_3m;
    if (timeframe === "5") return snapshot.candles_5m;
    return snapshot.candles_15m;
  }, [snapshot, timeframe]);

  useEffect(() => {
    if (!hostRef.current) return;

    const chart = createChart(hostRef.current, {
      autoSize: true,
      layout: {
        background: { type: ColorType.Solid, color: "#0b0f16" },
        textColor: "#aeb8c8",
      },
      grid: {
        vertLines: { color: "#151b25" },
        horzLines: { color: "#151b25" },
      },
      rightPriceScale: { borderColor: "#252d3a" },
      timeScale: { borderColor: "#252d3a", timeVisible: true, secondsVisible: false },
    });
    const series = chart.addSeries(CandlestickSeries, {
      upColor: "#35d399",
      downColor: "#ff5c75",
      borderVisible: false,
      wickUpColor: "#35d399",
      wickDownColor: "#ff5c75",
    });

    chartRef.current = chart;
    seriesRef.current = series;
    return () => {
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, []);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;
    series.setData(
      candles.map((candle) => ({
        time: Math.floor(candle.start_ms / 1000) as UTCTimestamp,
        open: candle.open,
        high: candle.high,
        low: candle.low,
        close: candle.close,
      })),
    );
  }, [candles]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;
    for (const line of signalLines.current) {
      series.removePriceLine(line);
    }
    signalLines.current = [];

    const signal = snapshot.active_signal;
    if (!signal) return;
    const entry = (signal.entry_low + signal.entry_high) / 2;
    signalLines.current = [
      series.createPriceLine({
        price: entry,
        title: `ENTRY · ${signal.side.toUpperCase()}`,
        color: "#e8edf7",
        lineWidth: 2,
        lineStyle: 0,
        axisLabelVisible: true,
      }),
      series.createPriceLine({
        price: signal.stop_loss,
        title: "SL",
        color: "#ff5c75",
        lineWidth: 2,
        lineStyle: 0,
        axisLabelVisible: true,
      }),
      series.createPriceLine({
        price: signal.tp1,
        title: "TP1",
        color: "#51b7ff",
        lineWidth: 1,
        lineStyle: 2,
        axisLabelVisible: true,
      }),
      series.createPriceLine({
        price: signal.tp2,
        title: "TP2",
        color: "#35d399",
        lineWidth: 2,
        lineStyle: 2,
        axisLabelVisible: true,
      }),
    ];
  }, [snapshot.active_signal]);

  return <div className="chart" ref={hostRef} />;
}

function SignalCard({ signal }: { signal: TradeSignal | null }) {
  if (!signal) {
    return (
      <section className="panel signal-card">
        <div className="eyebrow">DECISION ENGINE</div>
        <h2>NO TRADE</h2>
        <p className="muted">Waiting for the confluence threshold and risk gates.</p>
      </section>
    );
  }

  return (
    <section className="panel signal-card">
      <div className="signal-header">
        <div>
          <div className="eyebrow">ACTIVE SETUP</div>
          <h2 className={signal.side === "long" ? "positive" : "negative"}>
            {signal.side.toUpperCase()} · {signal.status.replaceAll("_", " ").toUpperCase()}
          </h2>
        </div>
        <div className="confidence">
          <strong>{(signal.confidence * 100).toFixed(1)}%</strong>
          <span>{signal.calibrated ? "calibrated probability" : "quality score"}</span>
        </div>
      </div>
      <div className="levels">
        <Metric label="ENTRY" value={`${signal.entry_low.toFixed(2)}–${signal.entry_high.toFixed(2)}`} />
        <Metric label="STOP" value={signal.stop_loss.toFixed(2)} />
        <Metric label="TP1" value={signal.tp1.toFixed(2)} />
        <Metric label="TP2" value={signal.tp2.toFixed(2)} />
        <Metric label="R:R TP2" value={`1:${signal.risk_reward_tp2.toFixed(2)}`} />
      </div>
      <div className="reason-list">
        {signal.reasons.map((reason) => (
          <span key={reason}>{reason}</span>
        ))}
      </div>
      <p className="invalidation">{signal.invalidation}</p>
    </section>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

export default function App() {
  const [snapshot, setSnapshot] = useState<EngineSnapshot | null>(null);
  const [events, setEvents] = useState<EngineEvent[]>([]);
  const [socketUp, setSocketUp] = useState(false);
  const [timeframe, setTimeframe] = useState<Timeframe>("3");
  const alarms = useAudioAlarms();
  const alarmPlayer = useRef(alarms.play);
  alarmPlayer.current = alarms.play;

  useEffect(() => {
    let socket: WebSocket | null = null;
    let retry: number | undefined;
    let stopped = false;

    const connect = () => {
      socket = new WebSocket(WS_URL);
      socket.onopen = () => setSocketUp(true);
      socket.onmessage = (message) => {
        const envelope = JSON.parse(message.data) as WsEnvelope;
        if (envelope.kind === "snapshot") {
          setSnapshot(envelope.data);
        } else {
          setEvents((current) => [envelope.data, ...current].slice(0, 30));
          if (envelope.data.alert) {
            alarmPlayer.current(envelope.data.alert);
          }
        }
      };
      socket.onerror = () => socket?.close();
      socket.onclose = () => {
        setSocketUp(false);
        if (!stopped) {
          retry = window.setTimeout(connect, 1000);
        }
      };
    };

    connect();
    return () => {
      stopped = true;
      if (retry) window.clearTimeout(retry);
      socket?.close();
    };
  }, []);

  if (!snapshot) {
    return (
      <main className="loading">
        <h1>AGENT-6</h1>
        <p>Connecting to local engine at {WS_URL}…</p>
        <p className="muted">Start with: cargo run -p agent6-engine</p>
      </main>
    );
  }

  const f = snapshot.features;

  return (
    <main className="app-shell">
      <header>
        <div>
          <div className="brand">AGENT-6</div>
          <div className="subtitle">LOCAL CRYPTO SCALPING INTELLIGENCE</div>
        </div>
        <div className="header-actions">
          <div className={`status ${socketUp && snapshot.connected ? "online" : "offline"}`}>
            <span />
            {socketUp && snapshot.connected ? "LIVE" : "RECONNECTING"}
          </div>
          <button className={alarms.enabled ? "button enabled" : "button"} onClick={alarms.enable}>
            {alarms.enabled ? "ALARMS ON" : "ENABLE ALARMS"}
          </button>
        </div>
      </header>

      <section className="ticker-row">
        <div>
          <span className="ticker">{snapshot.symbol}</span>
          <strong className="last-price">
            {snapshot.last_price?.toLocaleString(undefined, { maximumFractionDigits: 4 }) ?? "—"}
          </strong>
        </div>
        <div className="timeframes">
          {(["1", "3", "5", "15"] as Timeframe[]).map((tf) => (
            <button
              key={tf}
              className={timeframe === tf ? "tf active" : "tf"}
              onClick={() => setTimeframe(tf)}
            >
              {tf}m
            </button>
          ))}
        </div>
      </section>

      <div className="main-grid">
        <section className="panel chart-panel">
          <Chart snapshot={snapshot} timeframe={timeframe} />
          <div className="attribution">
            Charts powered by TradingView Lightweight Charts
          </div>
        </section>

        <div className="right-column">
          <SignalCard signal={snapshot.active_signal} />

          <section className="panel">
            <div className="eyebrow">LIVE FEATURE STACK</div>
            <div className="feature-grid">
              <Metric label="REGIME" value={f?.regime.replaceAll("_", " ").toUpperCase() ?? "WARMING"} />
              <Metric label="SPREAD" value={f ? `${f.spread_bps.toFixed(2)} bp` : "—"} />
              <Metric label="BOOK IMB." value={f ? f.book_imbalance.toFixed(3) : "—"} />
              <Metric label="FLOW IMB." value={f ? f.trade_flow_imbalance.toFixed(3) : "—"} />
              <Metric label="15m TREND" value={f ? `${f.trend_15m_bps.toFixed(1)} bp` : "—"} />
              <Metric label="5m TREND" value={f ? `${f.trend_5m_bps.toFixed(1)} bp` : "—"} />
              <Metric label="VWAP20" value={f ? f.vwap_20.toFixed(2) : "—"} />
              <Metric label="ATR14" value={f ? f.atr_14.toFixed(2) : "—"} />
              <Metric label="OI Δ" value={f ? `${f.open_interest_delta_pct.toFixed(3)}%` : "—"} />
              <Metric label="LIQ PRESS." value={f ? f.liquidation_pressure.toFixed(3) : "—"} />
              <Metric label="LONG SCORE" value={f ? `${(f.long_score * 100).toFixed(1)}%` : "—"} />
              <Metric label="SHORT SCORE" value={f ? `${(f.short_score * 100).toFixed(1)}%` : "—"} />
            </div>
          </section>
        </div>
      </div>

      <section className="panel event-panel">
        <div className="eyebrow">EVENT + ALARM JOURNAL</div>
        <div className="events">
          {events.length === 0 ? (
            <p className="muted">No key events since this dashboard connected.</p>
          ) : (
            events.map((event, index) => (
              <div className="event" key={`${event.ts_ms}-${event.event_type}-${index}`}>
                <time>{new Date(event.ts_ms).toLocaleTimeString()}</time>
                <strong>{event.event_type.replaceAll("_", " ").toUpperCase()}</strong>
                <span>{event.message}</span>
              </div>
            ))
          )}
        </div>
      </section>
    </main>
  );
}
