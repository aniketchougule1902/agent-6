import { useEffect, useMemo, useRef, useState } from "react";
import {
  CandlestickSeries,
  ColorType,
  createChart,
  createSeriesMarkers,
  LineSeries,
  HistogramSeries,
  type ISeriesMarkersPluginApi,
  type SeriesMarker,
  type Time,
  type IChartApi,
  type AutoscaleInfo,
  type IPriceLine,
  type ISeriesApi,
  type UTCTimestamp,
} from "lightweight-charts";
import type { AlertKind } from "./generated/AlertKind";
import type { EngineEvent } from "./generated/EngineEvent";
import type { EngineSnapshot } from "./generated/EngineSnapshot";
import type { TradeSignal } from "./generated/TradeSignal";
import type { RuntimeMarketUpdate } from "./generated/RuntimeMarketUpdate";
import { overlayData } from "./chartIndicators";
import { CoinIcon, ServiceBar, SymbolSearch, useCatalog, price } from "./MarketTools";
import { MarketSidebar } from "./MarketSidebar";
import { RiskPlanner } from "./RiskPlanner";
import { PaperTrading } from "./PaperTrading";
import { drawSetup } from "./drawSetup";
import { isOpenSetup, selectSetup } from "./setupSelection";

type Timeframe = "1" | "3" | "5" | "15";
type WsEnvelope =
  | { kind: "snapshot"; data: EngineSnapshot }
  | { kind: "event"; data: EngineEvent };

const WS_URL = import.meta.env.VITE_ENGINE_WS ?? "ws://127.0.0.1:8787/ws";
const MARKET_API = import.meta.env.VITE_MARKET_API ?? "/api/market";

function useAudioAlarms() {
  const context = useRef<AudioContext | null>(null);
  const [enabled, setEnabled] = useState(false);

  async function enable() {
    const audio = context.current ?? new AudioContext();
    context.current = audio;
    await audio.resume();
    setEnabled(true);
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
      feed_stale: [165, 0.32, 3],
      feed_recovered: [740, 0.10, 2],
      drift: [260, 0.22, 4],
      model_promoted: [1174, 0.13, 4],
    };

    const [frequency, duration, count] = patterns[kind];
    for (let i = 0; i < count; i += 1) {
      const start = audio.currentTime + i * (duration + 0.07);
      const oscillator = audio.createOscillator();
      const gain = audio.createGain();
      oscillator.type = kind === "stop_loss" || kind === "feed_stale" ? "sawtooth" : "sine";
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

function Chart({ snapshot, timeframe, tickSize }: { snapshot: EngineSnapshot; timeframe: Timeframe; tickSize?: string }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const signalLines = useRef<IPriceLine[]>([]);
  const markersRef = useRef<ISeriesMarkersPluginApi<Time> | null>(null);
  const overlays = useRef<Record<string, ISeriesApi<"Line">>>({});
  const volumeRef = useRef<ISeriesApi<"Histogram"> | null>(null);
  const chartMarket = useRef("");
  const priorCandleCount=useRef(0);
  const drawingRef = useRef<HTMLCanvasElement|null>(null);
  const [showRibbon,setShowRibbon]=useState(true);
  const [showZones,setShowZones]=useState(true);
  const [trendColors,setTrendColors]=useState(true);
  const [showEma, setShowEma] = useState(true);
  const [showBands, setShowBands] = useState(false);
  const [showVwap, setShowVwap] = useState(true);
  const [allFlags, setAllFlags] = useState(false);

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
        background: { type: ColorType.Solid, color: "#151823" },
        textColor: "#aeb8c8",
      },
      grid: { vertLines: { color: "#151b25" }, horzLines: { color: "#151b25" } },
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
    markersRef.current = createSeriesMarkers(series, [], { zOrder: "top" });
    for (const [key, color] of Object.entries({ema9:"#f7bb54",ema21:"#5c9eff",ema50:"#ac86f7",upper:"#4b667c",lower:"#4b667c",vwap:"#e88ee8"})) {
      overlays.current[key] = chart.addSeries(LineSeries, {color,lineWidth:1,priceLineVisible:false,lastValueVisible:false,crosshairMarkerVisible:false});
    }
    volumeRef.current = chart.addSeries(HistogramSeries, {priceFormat:{type:"volume"},priceLineVisible:false,lastValueVisible:false}, 1);
    chart.panes()[1].setHeight(100);
    return () => {
      markersRef.current?.detach();
      markersRef.current = null;
      signalLines.current = [];
      overlays.current = {};
      volumeRef.current = null;
      chartMarket.current = "";
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, []);

  const indicatorData=useMemo(()=>overlayData(candles),[candles]);
  useEffect(()=>{
    const chart=chartRef.current,series=seriesRef.current,canvas=drawingRef.current;
    if(!chart||!series||!canvas)return;
    let frame=0;let lastDraw=0;
    const draw=(now:number)=>{
      if(now-lastDraw>32){
        lastDraw=now;const rect=hostRef.current!.getBoundingClientRect();const dpr=window.devicePixelRatio||1;
        if(canvas.width!==Math.round(rect.width*dpr)||canvas.height!==Math.round(rect.height*dpr)){canvas.width=Math.round(rect.width*dpr);canvas.height=Math.round(rect.height*dpr);}
        const ctx=canvas.getContext('2d');if(ctx){ctx.setTransform(dpr,0,0,dpr,0,0);drawSetup(ctx,chart,series,candles,indicatorData,snapshot.active_signal,showRibbon,showZones,rect.width,rect.height);}
      }
      frame=requestAnimationFrame(draw);
    };
    frame=requestAnimationFrame(draw);return()=>cancelAnimationFrame(frame);
  },[candles,indicatorData,snapshot.active_signal,showRibbon,showZones]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;
    const minimum=Math.min(...candles.map(c=>c.low));
    const tick=Number(tickSize);
    const precision=tick>0?Math.max(0,(tickSize?.replace(/0+$/,'').split('.')[1]??'').length):minimum<0.01?10:minimum<1?7:minimum<100?5:2;
    series.applyOptions({priceFormat:{type:'price',precision,minMove:tick>0?tick:10**(-precision)}});
    const slow=new Map(indicatorData.ema50.map(p=>[p.time,p.value]));
    const fast=new Map(indicatorData.ema21.map(p=>[p.time,p.value]));
    series.setData(
      candles.map((candle) => ({
        time: Math.floor(candle.start_ms / 1000) as UTCTimestamp,
        open: candle.open,
        high: candle.high,
        low: candle.low,
        close: candle.close,
        ...(trendColors?{color: candle.close>(fast.get(Math.floor(candle.start_ms/1000) as UTCTimestamp)??candle.close)&&candle.close>(slow.get(Math.floor(candle.start_ms/1000) as UTCTimestamp)??candle.close)?'#35e1ab':candle.close<(fast.get(Math.floor(candle.start_ms/1000) as UTCTimestamp)??candle.close)&&candle.close<(slow.get(Math.floor(candle.start_ms/1000) as UTCTimestamp)??candle.close)?'#ff426a':'#a57aec',wickColor:candle.close>=candle.open?'#35d399':'#ff5c75'}:{}),
      })),
    );
    const data = indicatorData;
    for (const [key, overlay] of Object.entries(overlays.current)) {
      overlay.setData(data[key]);
    }
    volumeRef.current?.setData(candles.map(c => ({time:Math.floor(c.start_ms/1000) as UTCTimestamp,value:c.volume,color:c.close>=c.open?"#237c62":"#8c3547"})));
    const marketKey = `${snapshot.symbol}:${timeframe}`;
    if (candles.length && (chartMarket.current !== marketKey || (priorCandleCount.current<60&&candles.length>=60))) {
      chartRef.current?.timeScale().setVisibleLogicalRange({from:Math.max(0,candles.length-90),to:candles.length+6});
      chartMarket.current=marketKey;
    }
    priorCandleCount.current=candles.length;
  }, [candles, indicatorData, snapshot.symbol, timeframe, trendColors, tickSize]);

  useEffect(() => {
    for (const [key, overlay] of Object.entries(overlays.current)) {
      overlay.applyOptions({visible:key.startsWith("ema")?showEma:key==="vwap"?showVwap:showBands});
    }
  }, [showEma, showBands, showVwap]);

  useEffect(() => {
    const duration = Number(timeframe)*60_000;
    const times = new Set(candles.map(c => Math.floor(c.start_ms/1000)));
    const flags = (snapshot.chart_flags ?? []).filter(f => allFlags || f.timeframe === (snapshot.active_signal?.timeframe??timeframe))
      .map(f => ({...f,time:Math.floor(f.ts_ms/duration)*duration/1000 as UTCTimestamp}))
      .filter(f => times.has(f.time)).sort((a,b)=>a.time-b.time || a.ts_ms-b.ts_ms);
    const markers: SeriesMarker<Time>[] = flags.map(f => {
      const entry=f.kind==="signal";
      const label=entry?(f.side==="long"?"ENTRY LONG":"ENTRY SHORT"):f.kind==="stop_loss"?"SL EXIT":f.kind==="tp2"?"TP2 EXIT":f.kind==="tp1"?"TP1 TOUCH":"EXPIRED";
      return {time:f.time,id:f.id,price:f.price,position:"atPriceMiddle",shape:entry?(f.side==="long"?"arrowUp":"arrowDown"):"square",color:entry?"#f7bb54":f.kind==="stop_loss"?"#ff5c75":"#35d399",text:`${f.timeframe}m ${label} ${price(f.price)}`,size:1};
    });
    markersRef.current?.setMarkers(markers);
  }, [snapshot.chart_flags, snapshot.active_signal?.timeframe, candles, timeframe, allFlags]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;
    for (const line of signalLines.current) series.removePriceLine(line);
    signalLines.current = [];

    const signal = snapshot.active_signal;
    series.applyOptions({autoscaleInfoProvider:(original:()=>AutoscaleInfo|null)=>{
      const info=original();
      if(!info?.priceRange||!signal)return info;
      return {...info,priceRange:{minValue:Math.min(info.priceRange.minValue,signal.stop_loss,signal.tp1,signal.tp2,signal.entry_low),maxValue:Math.max(info.priceRange.maxValue,signal.stop_loss,signal.tp1,signal.tp2,signal.entry_high)}};
    }});
    if (!signal) return;
    const entry = (signal.entry_low + signal.entry_high) / 2;
    signalLines.current = [
      series.createPriceLine({
        price: entry,
        title: `${signal.timeframe}m ENTRY · ${signal.side.toUpperCase()}`,
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

  function downloadDrawing() {
    const chart = chartRef.current;
    if (!chart) return;
    const link = document.createElement("a");
    const screenshot=chart.takeScreenshot();
    if(drawingRef.current)screenshot.getContext('2d')?.drawImage(drawingRef.current,0,0,screenshot.width,screenshot.height);
    link.href = screenshot.toDataURL("image/png");
    link.download = `agent6-${snapshot.symbol}-${timeframe}m-${snapshot.active_signal?.id??"analysis"}.png`;
    link.click();
  }

  return <>
    <div className="chart-controls">
      <label><input type="checkbox" checked={showRibbon} onChange={e=>setShowRibbon(e.target.checked)}/> Trend ribbon</label>
      <label><input type="checkbox" checked={showZones} onChange={e=>setShowZones(e.target.checked)}/> TP / SL zones</label>
      <label><input type="checkbox" checked={trendColors} onChange={e=>setTrendColors(e.target.checked)}/> Trend candles</label>
      <label><input type="checkbox" checked={showEma} onChange={e=>setShowEma(e.target.checked)}/> EMA 9 / 21 / 50</label>
      <label><input type="checkbox" checked={showVwap} onChange={e=>setShowVwap(e.target.checked)}/> VWAP 20</label>
      <label><input type="checkbox" checked={showBands} onChange={e=>setShowBands(e.target.checked)}/> Bollinger 20, 2</label>
      <label><input type="checkbox" checked={allFlags} onChange={e=>setAllFlags(e.target.checked)}/> All timeframe flags</label>
    </div>
    <div className="chart-stage"><div className="chart" ref={hostRef}/><canvas ref={drawingRef} className="setup-canvas" aria-hidden="true"/><div className="chart-watermark">AGENT / 6 <small>{snapshot.symbol} ? {timeframe}m ? PAPER</small></div></div>
    <div className="muted chart-legend">Yellow arrows: paper entries · Green flags: target touches · Red flags: stop touches. Overlays update live; decisions use closed candles.</div>
    <button className="button chart-download" onClick={downloadDrawing}>
      Export chart PNG
    </button>
  </>;
}

function SignalCard({ signal, stale, chartTimeframe }: { signal: TradeSignal | null; stale: boolean; chartTimeframe?:string }) {
  if (!signal) {
    return (
      <section className="panel signal-card">
        <div className="eyebrow">DECISION ENGINE</div>
        <h2>{stale ? "DATA HALT" : "WAITING FOR A SETUP"}</h2>
        <p className="muted">
          {stale ? "Feed-integrity gate is suspending new setups." : "No admitted setup for this symbol yet. The timeframe cards explain which new-entry checks are waiting."}
        </p>
      </section>
    );
  }

  return (
    <section className="panel signal-card">
      <div className="signal-header">
        <div>
          <div className="eyebrow">{signal.timeframe}m {isOpenSetup(signal)?"OPEN PAPER SETUP":"LAST PAPER SETUP"}</div>
          <h2 className={signal.side === "long" ? "positive" : "negative"}>
            {signal.side.toUpperCase()} · {signal.status.replaceAll("_", " ").toUpperCase()}
          </h2>
        </div>
        <div className="confidence">
          <strong>{(signal.confidence * 100).toFixed(1)}%</strong>
          <span>{signal.calibrated ? "TP2 outcome probability" : "quality score"}</span>
        </div>
      </div>
      {chartTimeframe&&<p className="setup-context">Viewing {signal.timeframe}m setup on the {chartTimeframe}m chart. Entry recorded at {new Date(signal.created_at_ms).toLocaleTimeString()}; this is not a fresh entry recommendation.</p>}
      {stale&&<p className="invalidation">Feed unavailable: displayed levels are historical; live price monitoring is interrupted.</p>}
      <div className="levels">
        <Metric label="ENTRY" value={signal.entry_low===signal.entry_high?price(signal.entry_low):`${price(signal.entry_low)} - ${price(signal.entry_high)}`} />
        <Metric label="STOP" value={price(signal.stop_loss)} />
        <Metric label="TP1" value={price(signal.tp1)} />
        <Metric label="TP2" value={price(signal.tp2)} />
        <Metric label="R:R TP2" value={`1:${signal.risk_reward_tp2.toFixed(2)}`} />
      </div>
      <div className="reason-list">
        {signal.reasons.map((reason) => <span key={reason}>{reason}</span>)}
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

function LiveApp() {
  const [snapshot, setSnapshot] = useState<EngineSnapshot | null>(null);
  const [events, setEvents] = useState<EngineEvent[]>([]);
  const [socketUp, setSocketUp] = useState(false);
  const [timeframe, setTimeframe] = useState<Timeframe>("3");
  const {instruments,error:catalogError}=useCatalog();
  const [setupFocus,setSetupFocus]=useState("auto");
  const [marketUpdating, setMarketUpdating] = useState(false);
  const [marketError, setMarketError] = useState<string | null>(null);
  const alarms = useAudioAlarms();

  const alarmPlayer = useRef(alarms.play);
  alarmPlayer.current = alarms.play;

  async function updateMarket(update: RuntimeMarketUpdate) {
    setMarketUpdating(true);
    setMarketError(null);
    try {
      const response = await fetch(MARKET_API, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(update),
      });
      const payload = await response.json() as {
        symbol?: string;
        timeframe?: string;
        error?: string;
      };
      if (!response.ok) throw new Error(payload.error ?? "Market update failed");

      if (payload.timeframe) setTimeframe(payload.timeframe as Timeframe);
    } catch (error) {
      setMarketError(error instanceof Error ? error.message : "Market update failed");
    } finally {
      setMarketUpdating(false);
    }
  }

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
          setTimeframe(envelope.data.timeframe as Timeframe);

        } else {
          setEvents((current) => [envelope.data, ...current].slice(0, 30));
          if (envelope.data.alert) alarmPlayer.current(envelope.data.alert);
        }
      };
      socket.onerror = () => socket?.close();
      socket.onclose = () => {
        setSocketUp(false);
        if (!stopped) retry = window.setTimeout(connect, 1000);
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

  const signals=snapshot.timeframe_signals??[];
  const openSetups=signals.filter(isOpenSetup);
  const displayedSetup=selectSetup(signals,timeframe,setupFocus);
  const chartSnapshot={...snapshot,active_signal:displayedSetup};
  const a = snapshot.analyses?.find(item => item.timeframe === timeframe);
  const instrument=instruments.find(i=>i.symbol===snapshot.symbol);
  const f = snapshot.features;
  const healthy = socketUp && snapshot.connected && !snapshot.feed_stale && !!snapshot.features;
  const statusText = !socketUp || !snapshot.connected ? "RECONNECTING" : snapshot.feed_stale ? "STALE DATA" : !snapshot.features ? "WARMING" : "LIVE";

  return (
    <main className="app-shell">
      <ServiceBar socketUp={socketUp} alarms={alarms.enabled}/>
      <header>
        <div>
          <div className="brand">AGENT-6</div>
          <div className="subtitle">LOCAL CRYPTO SCALPING INTELLIGENCE</div>
        </div>
        <div className="header-actions">
          <div className={`status ${healthy ? "online" : "offline"}`}>
            <span />
            {statusText}
          </div>
          <button className={alarms.enabled ? "button enabled" : "button"} onClick={alarms.enable}>
            {alarms.enabled ? "ALARMS ON" : "ENABLE ALARMS"}
          </button>
        </div>
      </header>

      <div className="workspace"><MarketSidebar busy={marketUpdating} onSelect={symbol=>void updateMarket({symbol,timeframe:"5"})}/><div className="workspace-main">
      <section className="ticker-row">
        <div className="ticker-control">
          <SymbolSearch instruments={instruments} error={catalogError} busy={marketUpdating} symbol={snapshot.symbol} onSelect={symbol=>void updateMarket({symbol,timeframe:null})}/>
          <div>
            <CoinIcon base={instrument?.base??snapshot.symbol.replace(/USDT$|USDC$/,"")}/><span className="ticker">{snapshot.symbol}</span>
            <strong className="last-price">
              {snapshot.last_price!=null?price(snapshot.last_price): "—"}
            </strong>
            <a
              className="button"
              href={`https://www.tradingview.com/chart/?symbol=${encodeURIComponent(`BYBIT:${snapshot.symbol}.P`)}`}
              target="_blank"
              rel="noopener noreferrer"
            >
              Open in TradingView
            </a>
          </div>
          {marketError && <span className="market-error">{marketError}</span>}
        </div>
        <div className="timeframes">
          {(["1", "3", "5", "15"] as Timeframe[]).map((tf) => (
            <button
              key={tf}
              className={timeframe === tf ? "tf active" : "tf"}
              disabled={marketUpdating}
              onClick={() => void updateMarket({ symbol: null, timeframe: tf })}
            >
              {tf}m
            </button>
          ))}
        </div>
      </section>

      <section className="setup-strip panel" aria-label="Tracked setups">
        <div><strong>{openSetups.length} open paper setup{openSetups.length===1?'':'s'}</strong><small>Chart interval and setup interval can differ. Existing setups stay visible.</small></div>
        <label>Display setup <select aria-label="Display setup" value={signals.some(s=>s.timeframe===setupFocus)?setupFocus:"auto"} onChange={e=>setSetupFocus(e.target.value)}><option value="auto">Auto: prefer open setup</option>{signals.map(s=><option key={s.id} value={s.timeframe}>{s.timeframe}m {s.side.toUpperCase()} / {s.status.replaceAll('_',' ')}</option>)}</select></label>
        {displayedSetup&&<span className="setup-badge">{displayedSetup.timeframe}m {displayedSetup.side.toUpperCase()} | {isOpenSetup(displayedSetup)?'TRACKING':'COMPLETED'}</span>}
      </section>
      <section className="timeframe-board" aria-label="All scalping timeframes">
        {(["1", "3", "5", "15"] as Timeframe[]).map(tf => {
          const analysis = snapshot.analyses?.find(item => item.timeframe === tf);
          const signal = snapshot.timeframe_signals?.find(item => item.timeframe === tf);
          return <button key={tf} className={`panel timeframe-card ${tf === timeframe ? "selected" : ""}`} disabled={marketUpdating} onClick={()=>void updateMarket({symbol:null,timeframe:tf})}>
            <strong>{tf}m <span className={analysis?.bias === "long" ? "positive" : analysis?.bias === "short" ? "negative" : ""}>{analysis?.bias.toUpperCase() ?? "WARMING"}</span></strong>
            <span>{signal ? `${signal.side.toUpperCase()} / ${signal.status.replaceAll("_"," ")}` : "NO NEW ENTRY"}</span>
            <span>{analysis ? `New-entry quality ${(analysis.quality*100).toFixed(0)}% / ${analysis.setup.replaceAll("_"," ")}` : "Waiting for closed candles"}</span>
            <small>{signal&&isOpenSetup(signal)?'Existing setup is tracking TP/SL. ':''}{analysis?.blockers.length?`New entry blocked: ${analysis.blockers.join('; ')}`:analysis?'New-entry checks clear':'Loading closed candles'}</small>
          </button>;
        })}
      </section>
      <div className="main-grid">
        <section className="panel chart-panel">
          <div className="chart-setup-caption">{displayedSetup?`${displayedSetup.timeframe}m ${displayedSetup.side.toUpperCase()} setup levels on ${timeframe}m candles`:`${timeframe}m candles | waiting for an admitted setup`}</div>
          <Chart snapshot={chartSnapshot} timeframe={timeframe} tickSize={instrument?.tick_size} />
          <div className="attribution">Charts powered by TradingView Lightweight Charts</div>
        </section>

        <div className="right-column">
          <SignalCard signal={displayedSetup} stale={!healthy} chartTimeframe={timeframe} />
          <RiskPlanner signal={displayedSetup} instrument={instrument}/>
          <PaperTrading snapshot={snapshot} signal={displayedSetup}/>
          <details className="feature-details"><summary>Order flow & microstructure</summary>

          <section className="panel">
            <div className="eyebrow">LIVE FEATURE STACK</div>
            <div className="feature-grid">
              <Metric label="REGIME" value={f?.regime.replaceAll("_", " ").toUpperCase() ?? "WARMING"} />
              <Metric label="FEED AGE" value={f ? `${f.feed_age_ms} ms` : `${snapshot.feed_age_ms} ms`} />
              <Metric label="BOOK AGE" value={f ? `${f.orderbook_age_ms} ms` : "—"} />
              <Metric label="SPREAD" value={f ? `${f.spread_bps.toFixed(2)} bp` : "—"} />
              <Metric label="L50 IMB." value={f ? f.book_imbalance.toFixed(3) : "—"} />
              <Metric label="TOP5 IMB." value={f ? f.book_imbalance_top5.toFixed(3) : "—"} />
              <Metric label="MICROPRICE" value={f ? `${f.microprice_bps.toFixed(2)} bp` : "—"} />
              <Metric label="DEPTH PRESS." value={f ? f.depth_pressure.toFixed(3) : "—"} />
              <Metric label="FLOW IMB." value={f ? f.trade_flow_imbalance.toFixed(3) : "—"} />
              <Metric label="TRADES / SEC" value={f ? f.trade_velocity_5s.toFixed(1) : "—"} />
              <Metric label="LARGE FLOW" value={f ? f.large_trade_imbalance.toFixed(3) : "—"} />
              <Metric label="LIQ BURST" value={f ? f.liquidation_burst_5s.toFixed(3) : "—"} />
              <Metric label="15m TREND" value={f ? `${f.trend_15m_bps.toFixed(1)} bp` : "—"} />
              <Metric label="5m TREND" value={f ? `${f.trend_5m_bps.toFixed(1)} bp` : "—"} />
              <Metric label="1m VWAP20" value={f ? f.vwap_20.toFixed(2) : "—"} />
              <Metric label="1m ATR14" value={f ? f.atr_14.toFixed(2) : "—"} />
              <Metric label="OI TICK Δ" value={f ? `${f.open_interest_delta_pct.toFixed(3)}%` : "—"} />
              <Metric label="OI 1m Δ" value={f ? `${f.open_interest_delta_1m_pct.toFixed(3)}%` : "—"} />
              <Metric label="OI 5m Δ" value={f ? `${f.open_interest_delta_5m_pct.toFixed(3)}%` : "—"} />
              <Metric label="LIQ PRESS." value={f ? f.liquidation_pressure.toFixed(3) : "—"} />
              <Metric label="LONG SCORE" value={f ? `${(f.long_score * 100).toFixed(1)}%` : "—"} />
              <Metric label="SHORT SCORE" value={f ? `${(f.short_score * 100).toFixed(1)}%` : "—"} />
            </div>
          </section>
          </details>
        </div>
      </div>

      <section className="panel event-panel">
        <div className="eyebrow">{timeframe}m CLOSED-CANDLE INDICATORS</div>
        {a ? <>
          <div className="indicator-grid">
            {Object.entries({"EMA 9":a.ema9,"EMA 21":a.ema21,"EMA 50":a.ema50,"RSI 14":a.rsi14,"ADX 14":a.adx14,"MACD HIST":a.macd_histogram,"ATR 14":a.atr14,"VWAP 20":a.vwap20,"BB UPPER":a.bb_upper,"BB LOWER":a.bb_lower,"REL VOLUME":a.relative_volume,"SUPPORT":a.support,"RESISTANCE":a.resistance}).map(([label,value])=><Metric key={label} label={label} value={["RSI 14","ADX 14","REL VOLUME"].includes(label)?value.toFixed(2):price(value)}/>)}
          </div>
          <p className="muted">Closed candle: {new Date(a.candle_ms).toLocaleString()} | {a.blockers.length ? `Waiting: ${a.blockers.join("; ")}` : "Admission checks clear"}. Quality is not a measured win probability.</p>
        </> : <p className="muted">Loading at least 60 closed candles for this timeframe.</p>}
      </section>
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
      </div></div>
    </main>
  );
}


export default function App(){return <LiveApp/>;}
