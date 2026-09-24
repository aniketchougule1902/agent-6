"""Leakage-safe multi-timeframe research features.

Higher-timeframe bars become visible only at their completed bucket boundary.  A row
inside an unfinished bucket can therefore retrieve only earlier completed context.
This module is research-only and never runs on the Rust signal hot path.
"""
from __future__ import annotations

import math
from collections import defaultdict, deque

import polars as pl


def add_completed_timeframe_context(
    frame: pl.DataFrame,
    *,
    base_interval_ms: int,
    timeframe_multiples: tuple[int, ...] = (3, 5, 15),
    volatility_bars: int = 5,
) -> pl.DataFrame:
    """Attach causal completed-bar return/volatility context to point-in-time rows.

    ``ts_ms`` is the observation time and ``price`` is the price known at that time.
    For a multiple M, observations in bucket ``[k*M, (k+1)*M)`` are aggregated into
    one close, but that close is not eligible for retrieval until ``(k+1)*M``.
    Thus changing any future/incomplete-bucket value cannot alter earlier features.
    """
    if base_interval_ms <= 0:
        raise ValueError("base_interval_ms must be positive")
    if volatility_bars < 2:
        raise ValueError("volatility_bars must be at least 2")
    if not timeframe_multiples or any(m <= 1 for m in timeframe_multiples):
        raise ValueError("timeframe multiples must all be greater than 1")
    if len(set(timeframe_multiples)) != len(timeframe_multiples):
        raise ValueError("timeframe multiples must be unique")
    if "ts_ms" not in frame.columns or "price" not in frame.columns:
        raise ValueError("multi-timeframe context requires ts_ms and price")

    ts = frame.get_column("ts_ms").to_list()
    prices = frame.get_column("price").to_list()
    if any(t is None or not isinstance(t, int) or t < 0 for t in ts):
        raise ValueError("ts_ms must contain non-negative integers")
    if any(ts[i] > ts[i + 1] for i in range(len(ts) - 1)):
        raise ValueError("multi-timeframe input must be chronological")
    if any(p is None or not math.isfinite(float(p)) or float(p) <= 0.0 for p in prices):
        raise ValueError("price must be finite and positive")

    out = frame
    for multiple in timeframe_multiples:
        interval = base_interval_ms * multiple
        closes: dict[int, float] = {}
        for t, p in zip(ts, prices, strict=True):
            bucket_end = (t // interval + 1) * interval
            closes[bucket_end] = float(p)

        completed = sorted(closes.items())
        feature_at: dict[int, tuple[float | None, float | None]] = {}
        prior_close: float | None = None
        returns: deque[float] = deque(maxlen=volatility_bars)
        for available_at, close in completed:
            ret = None if prior_close is None else close / prior_close - 1.0
            if ret is not None:
                returns.append(ret)
            vol = None
            if len(returns) >= 2:
                mean = sum(returns) / len(returns)
                vol = math.sqrt(sum((x - mean) ** 2 for x in returns) / (len(returns) - 1))
            feature_at[available_at] = (ret, vol)
            prior_close = close

        available_times = sorted(feature_at)
        cursor = -1
        row_returns: list[float | None] = []
        row_vols: list[float | None] = []
        for t in ts:
            while cursor + 1 < len(available_times) and available_times[cursor + 1] <= t:
                cursor += 1
            if cursor < 0:
                row_returns.append(None)
                row_vols.append(None)
            else:
                r, v = feature_at[available_times[cursor]]
                row_returns.append(r)
                row_vols.append(v)

        out = out.with_columns(
            pl.Series(f"mtf_{multiple}x_return_1", row_returns, dtype=pl.Float64),
            pl.Series(f"mtf_{multiple}x_realized_vol", row_vols, dtype=pl.Float64),
        )
    return out
