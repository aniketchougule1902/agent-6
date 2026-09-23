"""Causal market-regime labels for research datasets.

Labels use only observations at or before each row. They are descriptive features,
not trading instructions, and intentionally avoid centered/future windows.
"""
from __future__ import annotations

import numpy as np

REGIME_NAMES = ("warmup", "quiet", "trend_up", "trend_down", "volatile")


def causal_regime_labels(
    close: np.ndarray,
    *,
    lookback: int = 20,
    trend_threshold: float = 1.0,
    volatile_quantile: float = 0.75,
) -> np.ndarray:
    """Return deterministic causal regime labels from close prices.

    Trend strength is cumulative log return divided by realized volatility over
    the trailing window. Volatile classification uses the expanding quantile of
    *past* realized-volatility observations, so a future row cannot change a
    historical label.
    """
    prices = np.asarray(close, dtype=float)
    if prices.ndim != 1 or prices.size == 0:
        raise ValueError("close must be a non-empty 1D array")
    if not np.all(np.isfinite(prices)) or np.any(prices <= 0):
        raise ValueError("close must contain finite positive prices")
    if lookback < 3:
        raise ValueError("lookback must be >= 3")
    if not np.isfinite(trend_threshold) or trend_threshold <= 0:
        raise ValueError("trend_threshold must be finite and > 0")
    if not 0.5 <= volatile_quantile < 1.0:
        raise ValueError("volatile_quantile must be in [0.5, 1.0)")

    labels = np.full(prices.size, "warmup", dtype=object)
    log_returns = np.diff(np.log(prices))
    realized_vols: list[float] = []

    for i in range(lookback, prices.size):
        window = log_returns[i - lookback : i]
        vol = float(np.std(window, ddof=1))
        cumulative = float(np.sum(window))
        history = np.asarray(realized_vols, dtype=float)
        volatile_cutoff = (
            float(np.quantile(history, volatile_quantile))
            if history.size >= lookback
            else float("inf")
        )

        if vol > volatile_cutoff:
            label = "volatile"
        else:
            denom = vol * np.sqrt(lookback)
            strength = cumulative / denom if denom > 0 else 0.0
            if strength >= trend_threshold:
                label = "trend_up"
            elif strength <= -trend_threshold:
                label = "trend_down"
            else:
                label = "quiet"
        labels[i] = label
        realized_vols.append(vol)

    return labels
