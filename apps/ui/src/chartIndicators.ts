import type { Candle } from "./generated/Candle";
import type { UTCTimestamp } from "lightweight-charts";

export function overlayData(candles: Candle[]) {
  const result: Record<string, { time: UTCTimestamp; value: number }[]> = {
    ema9: [], ema21: [], ema50: [], upper: [], lower: [], vwap: [],
  };
  const ema = new Map<number, number>();
  for (let i = 0; i < candles.length; i++) {
    const candle = candles[i];
    const time = Math.floor(candle.start_ms / 1000) as UTCTimestamp;
    for (const period of [9, 21, 50]) {
      if (i + 1 < period) continue;
      const previous = ema.get(period);
      const value = previous === undefined
        ? candles.slice(i + 1 - period, i + 1).reduce((sum, b) => sum + b.close, 0) / period
        : previous + 2 / (period + 1) * (candle.close - previous);
      ema.set(period, value);
      result[`ema${period}`].push({ time, value });
    }
    if (i < 19) continue;
    const window = candles.slice(i - 19, i + 1);
    const mean = window.reduce((sum, b) => sum + b.close, 0) / 20;
    const sd = Math.sqrt(window.reduce((sum, b) => sum + (b.close - mean) ** 2, 0) / 20);
    result.upper.push({ time, value: mean + 2 * sd });
    result.lower.push({ time, value: mean - 2 * sd });
    const volume = window.reduce((sum, b) => sum + b.volume, 0);
    if (volume > 0) result.vwap.push({ time, value: window.reduce((sum, b) => sum + (b.high + b.low + b.close) / 3 * b.volume, 0) / volume });
  }
  return result;
}
