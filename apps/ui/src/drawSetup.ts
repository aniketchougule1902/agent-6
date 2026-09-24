import type { IChartApi, ISeriesApi, UTCTimestamp } from "lightweight-charts";
import type { Candle } from "./generated/Candle";
import type { TradeSignal } from "./generated/TradeSignal";
import { overlayData } from "./chartIndicators";
import { price } from "./MarketTools";

export type DrawingOptions = {
  trendlines: boolean;
  supportResistance: boolean;
  patterns: boolean;
  expectedPath: boolean;
  pivots: boolean;
};

type PivotKind = "high" | "low";
type Pivot = { index: number; price: number; kind: PivotKind };
type FittedLine = {
  kind: PivotKind;
  slope: number;
  intercept: number;
  firstIndex: number;
  lastIndex: number;
  touches: number;
  rmse: number;
};
type Level = { price: number; touches: number; kind: "support" | "resistance" };
type PatternName =
  | "rising_wedge"
  | "falling_wedge"
  | "symmetrical_triangle"
  | "ascending_triangle"
  | "descending_triangle"
  | "channel";
type Pattern = {
  name: PatternName;
  upper: FittedLine;
  lower: FittedLine;
  startIndex: number;
  bias: -1 | 0 | 1;
  quality: number;
  measuredMove: number;
};

const DEFAULT_OPTIONS: DrawingOptions = {
  trendlines: true,
  supportResistance: true,
  patterns: true,
  expectedPath: true,
  pivots: false,
};

function clamp(value: number, low: number, high: number) {
  return Math.max(low, Math.min(high, value));
}

function valueAt(line: FittedLine, index: number) {
  return line.intercept + line.slope * index;
}

function averageTrueRange(candles: Candle[], period = 14) {
  if (candles.length < 2) return 0;
  const start = Math.max(1, candles.length - period);
  let total = 0;
  let count = 0;
  for (let i = start; i < candles.length; i += 1) {
    const current = candles[i];
    const previous = candles[i - 1];
    total += Math.max(
      current.high - current.low,
      Math.abs(current.high - previous.close),
      Math.abs(current.low - previous.close),
    );
    count += 1;
  }
  return count ? total / count : 0;
}

function findPivots(candles: Candle[], span = 3): Pivot[] {
  const result: Pivot[] = [];
  if (candles.length < span * 2 + 1) return result;
  for (let i = span; i < candles.length - span; i += 1) {
    const current = candles[i];
    let high = true;
    let low = true;
    for (let j = i - span; j <= i + span; j += 1) {
      if (j === i) continue;
      if (candles[j].high >= current.high) high = false;
      if (candles[j].low <= current.low) low = false;
    }
    if (high) result.push({ index: i, price: current.high, kind: "high" });
    if (low) result.push({ index: i, price: current.low, kind: "low" });
  }
  return result;
}

function fitLine(points: Pivot[], kind: PivotKind): FittedLine | null {
  const selected = points.filter((point) => point.kind === kind).slice(-6);
  if (selected.length < 2) return null;
  const n = selected.length;
  const meanX = selected.reduce((sum, point) => sum + point.index, 0) / n;
  const meanY = selected.reduce((sum, point) => sum + point.price, 0) / n;
  let numerator = 0;
  let denominator = 0;
  for (const point of selected) {
    numerator += (point.index - meanX) * (point.price - meanY);
    denominator += (point.index - meanX) * (point.index - meanX);
  }
  if (denominator <= Number.EPSILON) return null;
  const slope = numerator / denominator;
  const intercept = meanY - slope * meanX;
  const rmse = Math.sqrt(
    selected.reduce((sum, point) => {
      const residual = point.price - (intercept + slope * point.index);
      return sum + residual * residual;
    }, 0) / n,
  );
  return {
    kind,
    slope,
    intercept,
    firstIndex: selected[0].index,
    lastIndex: selected[selected.length - 1].index,
    touches: n,
    rmse,
  };
}

function clusterLevels(points: Pivot[], current: number, tolerance: number): Level[] {
  if (!(tolerance > 0)) return [];
  const clusters: Array<{ sum: number; count: number; highs: number; lows: number }> = [];
  for (const point of points.slice(-32)) {
    let best = -1;
    let bestDistance = Infinity;
    for (let i = 0; i < clusters.length; i += 1) {
      const center = clusters[i].sum / clusters[i].count;
      const distance = Math.abs(center - point.price);
      if (distance <= tolerance && distance < bestDistance) {
        best = i;
        bestDistance = distance;
      }
    }
    if (best < 0) {
      clusters.push({
        sum: point.price,
        count: 1,
        highs: point.kind === "high" ? 1 : 0,
        lows: point.kind === "low" ? 1 : 0,
      });
    } else {
      const cluster = clusters[best];
      cluster.sum += point.price;
      cluster.count += 1;
      if (point.kind === "high") cluster.highs += 1;
      else cluster.lows += 1;
    }
  }
  return clusters
    .filter((cluster) => cluster.count >= 2)
    .map((cluster) => {
      const level = cluster.sum / cluster.count;
      return {
        price: level,
        touches: cluster.count,
        kind: level <= current || cluster.lows > cluster.highs ? "support" : "resistance",
      } as Level;
    })
    .sort(
      (a, b) =>
        b.touches - a.touches ||
        Math.abs(a.price - current) - Math.abs(b.price - current),
    )
    .slice(0, 5);
}

function detectPattern(
  upper: FittedLine | null,
  lower: FittedLine | null,
  candles: Candle[],
  atr: number,
): Pattern | null {
  if (!upper || !lower || !(atr > 0) || candles.length < 20) return null;
  const lastIndex = candles.length - 1;
  const startIndex = Math.max(upper.firstIndex, lower.firstIndex);
  if (lastIndex - startIndex < 6) return null;

  const startGap = valueAt(upper, startIndex) - valueAt(lower, startIndex);
  const endGap = valueAt(upper, lastIndex) - valueAt(lower, lastIndex);
  if (!(startGap > 0) || !(endGap > 0)) return null;

  const flat = atr * 0.025;
  const converging = endGap < startGap * 0.88;
  const u = upper.slope;
  const l = lower.slope;
  let name: PatternName | null = null;
  let bias: -1 | 0 | 1 = 0;

  if (converging && Math.abs(u) <= flat && l > flat) {
    name = "ascending_triangle";
    bias = 1;
  } else if (converging && u < -flat && Math.abs(l) <= flat) {
    name = "descending_triangle";
    bias = -1;
  } else if (converging && u < -flat && l > flat) {
    name = "symmetrical_triangle";
  } else if (converging && u > flat && l > flat && l > u) {
    name = "rising_wedge";
    bias = -1;
  } else if (converging && u < -flat && l < -flat && u < l) {
    name = "falling_wedge";
    bias = 1;
  } else if (
    Math.sign(u) === Math.sign(l) &&
    Math.abs(u - l) <= flat * 1.5
  ) {
    name = "channel";
  }

  if (!name) return null;
  const fitError = (upper.rmse + lower.rmse) / 2;
  const fitQuality = 1 - clamp(fitError / (atr * 0.55), 0, 1);
  const touchQuality = clamp((Math.min(upper.touches, lower.touches) - 2) / 3, 0, 1);
  const convergenceQuality = converging ? clamp(1 - endGap / startGap, 0, 1) : 0.45;
  const quality = clamp(
    0.45 * fitQuality + 0.30 * touchQuality + 0.25 * convergenceQuality,
    0,
    1,
  );
  if (quality < 0.35) return null;

  return {
    name,
    upper,
    lower,
    startIndex,
    bias,
    quality,
    measuredMove: clamp(startGap, atr, atr * 3),
  };
}

function analyzeStructure(candles: Candle[]) {
  const points = findPivots(candles);
  const atr = averageTrueRange(candles);
  const current = candles.length ? candles[candles.length - 1].close : 0;
  const upper = fitLine(points, "high");
  const lower = fitLine(points, "low");
  return {
    pivots: points,
    atr,
    upper,
    lower,
    levels: clusterLevels(points, current, Math.max(atr * 0.30, current * 0.00045)),
    pattern: detectPattern(upper, lower, candles, atr),
  };
}

function patternLabel(name: PatternName) {
  return name.replaceAll("_", " ").toUpperCase();
}

function label(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  text: string,
  color: string,
) {
  ctx.save();
  ctx.font = "800 10px system-ui";
  const boxWidth = ctx.measureText(text).width + 14;
  ctx.fillStyle = "rgba(11,15,22,.88)";
  ctx.fillRect(x, y - 14, boxWidth, 18);
  ctx.strokeStyle = color;
  ctx.globalAlpha = 0.65;
  ctx.strokeRect(x + 0.5, y - 13.5, boxWidth - 1, 17);
  ctx.globalAlpha = 1;
  ctx.fillStyle = color;
  ctx.fillText(text, x + 7, y - 1);
  ctx.restore();
}

function smoothPath(
  ctx: CanvasRenderingContext2D,
  points: Array<{ x: number; y: number }>,
  color: string,
  width: number,
  dash: number[],
) {
  if (points.length < 2) return;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.setLineDash(dash);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(points[0].x, points[0].y);
  for (let i = 1; i < points.length - 1; i += 1) {
    const current = points[i];
    const next = points[i + 1];
    ctx.quadraticCurveTo(
      current.x,
      current.y,
      (current.x + next.x) / 2,
      (current.y + next.y) / 2,
    );
  }
  ctx.lineTo(points[points.length - 1].x, points[points.length - 1].y);
  ctx.stroke();
  ctx.restore();
}

function arrowHead(
  ctx: CanvasRenderingContext2D,
  from: { x: number; y: number },
  to: { x: number; y: number },
  color: string,
) {
  const angle = Math.atan2(to.y - from.y, to.x - from.x);
  const size = 7;
  ctx.save();
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(to.x, to.y);
  ctx.lineTo(
    to.x - size * Math.cos(angle - Math.PI / 6),
    to.y - size * Math.sin(angle - Math.PI / 6),
  );
  ctx.lineTo(
    to.x - size * Math.cos(angle + Math.PI / 6),
    to.y - size * Math.sin(angle + Math.PI / 6),
  );
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

export function drawSetup(
  ctx: CanvasRenderingContext2D,
  chart: IChartApi,
  series: ISeriesApi<"Candlestick">,
  candles: Candle[],
  data: ReturnType<typeof overlayData>,
  signal: TradeSignal | null,
  ribbon: boolean,
  zones: boolean,
  width: number,
  height: number,
  drawingOptions: Partial<DrawingOptions> = {},
) {
  const options = { ...DEFAULT_OPTIONS, ...drawingOptions };
  ctx.clearRect(0, 0, width, height);
  const paneHeight = chart.panes()[0]?.getHeight() ?? height;
  const plotWidth = chart.timeScale().width();
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, plotWidth, paneHeight);
  ctx.clip();

  const x = (time: UTCTimestamp) => chart.timeScale().timeToCoordinate(time);
  const y = (value: number) => series.priceToCoordinate(value);
  const structure = analyzeStructure(candles);
  const lastIndex = candles.length - 1;
  const lastTime =
    lastIndex >= 0
      ? (Math.floor(candles[lastIndex].start_ms / 1000) as UTCTimestamp)
      : null;
  const previousTime =
    lastIndex > 0
      ? (Math.floor(candles[lastIndex - 1].start_ms / 1000) as UTCTimestamp)
      : null;
  const lastX = lastTime !== null ? x(lastTime) : null;
  const previousX = previousTime !== null ? x(previousTime) : null;
  const barWidth =
    lastX !== null && previousX !== null
      ? Math.max(5, Math.abs(lastX - previousX))
      : 10;

  const xIndex = (index: number) => {
    if (index >= 0 && index < candles.length) {
      return x(Math.floor(candles[index].start_ms / 1000) as UTCTimestamp);
    }
    if (lastX === null) return null;
    return lastX + (index - lastIndex) * barWidth;
  };

  if (ribbon) {
    const fast = new Map(data.ema21.map((point) => [point.time, point.value]));
    for (let i = 1; i < data.ema50.length; i += 1) {
      const a = data.ema50[i - 1];
      const b = data.ema50[i];
      const fastA = fast.get(a.time);
      const fastB = fast.get(b.time);
      if (fastA === undefined || fastB === undefined) continue;
      const xa = x(a.time);
      const xb = x(b.time);
      const ya = y(a.value);
      const yb = y(b.value);
      const yfa = y(fastA);
      const yfb = y(fastB);
      if (
        xa === null ||
        xb === null ||
        ya === null ||
        yb === null ||
        yfa === null ||
        yfb === null ||
        xb < 0 ||
        xa > plotWidth
      ) continue;
      ctx.fillStyle =
        fastB >= b.value ? "rgba(38,174,219,0.20)" : "rgba(230,61,109,0.20)";
      ctx.beginPath();
      ctx.moveTo(xa, ya);
      ctx.lineTo(xb, yb);
      ctx.lineTo(xb, yfb);
      ctx.lineTo(xa, yfa);
      ctx.closePath();
      ctx.fill();
    }
  }

  if (options.supportResistance && structure.levels.length && structure.atr > 0) {
    for (const level of structure.levels) {
      const py = y(level.price);
      const top = y(level.price + structure.atr * 0.06);
      const bottom = y(level.price - structure.atr * 0.06);
      if (py === null) continue;
      if (top !== null && bottom !== null) {
        ctx.fillStyle =
          level.kind === "support"
            ? "rgba(53,211,153,.055)"
            : "rgba(255,92,117,.055)";
        ctx.fillRect(0, Math.min(top, bottom), plotWidth, Math.abs(bottom - top));
      }
      ctx.save();
      ctx.strokeStyle =
        level.kind === "support"
          ? "rgba(53,211,153,.48)"
          : "rgba(255,92,117,.48)";
      ctx.lineWidth = 1;
      ctx.setLineDash([2, 5]);
      ctx.beginPath();
      ctx.moveTo(0, py);
      ctx.lineTo(plotWidth, py);
      ctx.stroke();
      ctx.restore();
      label(
        ctx,
        Math.max(6, plotWidth - 118),
        py - 3,
        (level.kind === "support" ? "S" : "R") + " · " + level.touches + " touches",
        level.kind === "support" ? "#62dcae" : "#ff8296",
      );
    }
  }

  const trendEnd = lastIndex + 9;
  const drawTrend = (line: FittedLine | null, color: string, title: string) => {
    if (!line) return;
    const start = Math.max(0, line.firstIndex - 2);
    const x1 = xIndex(start);
    const x2 = xIndex(trendEnd);
    const y1 = y(valueAt(line, start));
    const y2 = y(valueAt(line, trendEnd));
    if (x1 === null || x2 === null || y1 === null || y2 === null) return;
    ctx.save();
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.setLineDash([8, 4]);
    ctx.globalAlpha = 0.85;
    ctx.beginPath();
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
    ctx.stroke();
    ctx.restore();
    label(
      ctx,
      clamp(x2 - 94, 6, plotWidth - 105),
      clamp(y2, 18, paneHeight - 8),
      title,
      color,
    );
  };

  if (options.trendlines) {
    drawTrend(structure.upper, "#ff8a9d", "AUTO R-TREND");
    drawTrend(structure.lower, "#57d9ac", "AUTO S-TREND");
  }

  if (options.pivots) {
    const recent = structure.pivots.slice(-18);
    let priorHigh: number | null = null;
    let priorLow: number | null = null;
    for (const pivot of recent) {
      const px = xIndex(pivot.index);
      const py = y(pivot.price);
      if (px === null || py === null) continue;
      const prior = pivot.kind === "high" ? priorHigh : priorLow;
      let tag = pivot.kind === "high" ? "H" : "L";
      if (prior !== null) {
        tag =
          pivot.kind === "high"
            ? pivot.price > prior
              ? "HH"
              : "LH"
            : pivot.price > prior
              ? "HL"
              : "LL";
      }
      if (pivot.kind === "high") priorHigh = pivot.price;
      else priorLow = pivot.price;
      ctx.save();
      ctx.fillStyle = pivot.kind === "high" ? "#ff8a9d" : "#57d9ac";
      ctx.beginPath();
      ctx.arc(px, py, 3, 0, Math.PI * 2);
      ctx.fill();
      ctx.font = "800 9px system-ui";
      ctx.fillText(tag, px + 5, py + (pivot.kind === "high" ? -5 : 12));
      ctx.restore();
    }
  }

  if (options.patterns && structure.pattern) {
    const pattern = structure.pattern;
    const start = pattern.startIndex;
    const end = lastIndex + 7;
    const sx = xIndex(start);
    const ex = xIndex(end);
    const upperStart = y(valueAt(pattern.upper, start));
    const lowerStart = y(valueAt(pattern.lower, start));
    const upperEnd = y(valueAt(pattern.upper, end));
    const lowerEnd = y(valueAt(pattern.lower, end));
    if (
      sx !== null &&
      ex !== null &&
      upperStart !== null &&
      lowerStart !== null &&
      upperEnd !== null &&
      lowerEnd !== null
    ) {
      ctx.save();
      ctx.fillStyle = "rgba(247,185,85,.055)";
      ctx.strokeStyle = "rgba(247,185,85,.75)";
      ctx.lineWidth = 1.4;
      ctx.beginPath();
      ctx.moveTo(sx, upperStart);
      ctx.lineTo(ex, upperEnd);
      ctx.lineTo(ex, lowerEnd);
      ctx.lineTo(sx, lowerStart);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
      ctx.restore();
      label(
        ctx,
        clamp(sx + 8, 6, plotWidth - 190),
        clamp(Math.min(upperStart, lowerStart) - 4, 20, paneHeight - 12),
        patternLabel(pattern.name) + " · " + Math.round(pattern.quality * 100) + "%",
        "#f7c36d",
      );
      if (pattern.bias !== 0 && candles.length) {
        const measuredTarget =
          candles[candles.length - 1].close + pattern.bias * pattern.measuredMove;
        const measuredY = y(measuredTarget);
        if (measuredY !== null) {
          ctx.save();
          ctx.strokeStyle = "rgba(247,195,109,.55)";
          ctx.setLineDash([4, 5]);
          ctx.beginPath();
          ctx.moveTo(Math.max(0, lastX ?? 0), measuredY);
          ctx.lineTo(Math.min(plotWidth, ex), measuredY);
          ctx.stroke();
          ctx.restore();
          label(
            ctx,
            clamp((lastX ?? 0) + 8, 6, plotWidth - 140),
            clamp(measuredY - 3, 18, paneHeight - 8),
            "MEASURED " + price(measuredTarget),
            "#f7c36d",
          );
        }
      }
    }
  }

  if (zones && signal && candles.length) {
    const last = candles[candles.length - 1];
    const duration =
      candles.length > 1
        ? last.start_ms - candles[candles.length - 2].start_ms
        : 60000;
    const start = Math.floor(signal.created_at_ms / duration) * duration;
    const ended = ["tp2_hit", "stop_loss_hit", "expired", "reversed", "invalidated"].includes(
      signal.status,
    );
    const end =
      signal.last_event_ms > signal.created_at_ms && ended
        ? signal.last_event_ms
        : last.start_ms;
    const left = x(Math.floor(start / 1000) as UTCTimestamp);
    const right = x(
      Math.floor(Math.floor(end / duration) * duration / 1000) as UTCTimestamp,
    );
    const entry = (signal.entry_low + signal.entry_high) / 2;
    const entryY = y(entry);
    const stopY = y(signal.stop_loss);
    const targetY = y(signal.tp2);
    if (left !== null && entryY !== null && stopY !== null && targetY !== null) {
      const l = Math.max(0, left);
      const r = Math.min(plotWidth, Math.max(l + 120, (right ?? plotWidth - 15) + 35));
      ctx.fillStyle = "rgba(27,205,148,.16)";
      ctx.fillRect(l, Math.min(entryY, targetY), r - l, Math.abs(targetY - entryY));
      ctx.fillStyle = "rgba(246,72,103,.18)";
      ctx.fillRect(l, Math.min(entryY, stopY), r - l, Math.abs(stopY - entryY));
      const levelLabels: Array<[number, string, string]> = [
        [targetY, "TP2 " + price(signal.tp2) + " · " + signal.risk_reward_tp2.toFixed(1) + "R", "#35d399"],
        [entryY, "ENTRY " + price(entry), "#d8e4f7"],
        [stopY, "SL " + price(signal.stop_loss), "#ff5c75"],
      ];
      for (const [py, text, color] of levelLabels) {
        ctx.strokeStyle = color;
        ctx.setLineDash([5, 4]);
        ctx.beginPath();
        ctx.moveTo(l, py);
        ctx.lineTo(r, py);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.font = "bold 11px system-ui";
        const boxWidth = ctx.measureText(text).width + 16;
        ctx.fillStyle = "#111827";
        ctx.fillRect(Math.max(0, r - boxWidth), py - 20, boxWidth, 19);
        ctx.fillStyle = color;
        ctx.fillText(text, Math.max(0, r - boxWidth) + 8, py - 7);
      }
    }
  }

  if (
    options.expectedPath &&
    candles.length >= 20 &&
    structure.atr > 0 &&
    lastX !== null
  ) {
    const current = candles[candles.length - 1].close;
    const ema21 = data.ema21.length ? data.ema21[data.ema21.length - 1].value : current;
    const ema50 = data.ema50.length ? data.ema50[data.ema50.length - 1].value : current;
    const signalOpen = Boolean(
      signal && ["active", "tp1_hit"].includes(signal.status),
    );
    let direction: 1 | -1 =
      signalOpen && signal
        ? signal.side === "long"
          ? 1
          : -1
        : structure.pattern?.bias === 1
          ? 1
          : structure.pattern?.bias === -1
            ? -1
            : ema21 >= ema50
              ? 1
              : -1;

    let target1 = current + direction * structure.atr * 0.85;
    let target2 = current + direction * structure.atr * 1.65;
    if (signalOpen && signal) {
      target1 = signal.tp1;
      target2 = signal.tp2;
    } else if (structure.pattern && structure.pattern.bias !== 0) {
      target2 = current + direction * structure.pattern.measuredMove;
      target1 = current + (target2 - current) * 0.52;
    }

    const rawPath = [
      { offset: 0, value: current },
      { offset: 1.0, value: current - direction * structure.atr * 0.10 },
      { offset: 2.2, value: current + (target1 - current) * 0.42 },
      { offset: 3.5, value: target1 },
      { offset: 4.7, value: target1 - direction * structure.atr * 0.08 },
      { offset: 7.2, value: target2 },
    ];
    const projected: Array<{ x: number; y: number }> = [];
    for (const point of rawPath) {
      const py = y(point.value);
      if (py !== null) {
        projected.push({ x: lastX + point.offset * barWidth, y: py });
      }
    }
    if (projected.length >= 2) {
      const color = direction > 0 ? "#55e4b2" : "#ff718b";
      smoothPath(ctx, projected, color, 2.2, [2, 7]);
      arrowHead(
        ctx,
        projected[projected.length - 2],
        projected[projected.length - 1],
        color,
      );
      const end = projected[projected.length - 1];
      label(
        ctx,
        clamp(end.x - 156, 6, plotWidth - 166),
        clamp(end.y - 5, 18, paneHeight - 8),
        "EXPECTED PATH · SCENARIO",
        color,
      );
    }

    if (signalOpen && signal) {
      const startY = y(current);
      const invalidationY = y(signal.stop_loss);
      if (startY !== null && invalidationY !== null) {
        const invalidation = [
          { x: lastX, y: startY },
          { x: lastX + barWidth * 2.8, y: invalidationY },
        ];
        smoothPath(ctx, invalidation, "rgba(255,92,117,.65)", 1.2, [1, 6]);
        label(
          ctx,
          clamp(invalidation[1].x - 66, 6, plotWidth - 86),
          clamp(invalidation[1].y - 4, 18, paneHeight - 8),
          "INVALIDATION",
          "#ff8296",
        );
      }
    }
  }

  ctx.restore();
}
