"""Leakage-safe probability drift diagnostics for shadow/paper model monitoring."""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class ProbabilityDriftReport:
    psi: float
    mean_shift: float
    abstention_shift: float
    drifted: bool


def _probabilities(values: np.ndarray | list[float], name: str) -> np.ndarray:
    arr = np.asarray(values, dtype=float)
    if arr.ndim != 1 or arr.size == 0:
        raise ValueError(f"{name} must be a non-empty 1D probability vector")
    if not np.isfinite(arr).all() or np.any((arr < 0.0) | (arr > 1.0)):
        raise ValueError(f"{name} must contain finite probabilities in [0, 1]")
    return arr


def population_stability_index(
    reference: np.ndarray | list[float],
    current: np.ndarray | list[float],
    *,
    bins: int = 10,
    epsilon: float = 1e-6,
) -> float:
    """Return PSI using fixed probability bins learned independently of outcomes.

    Fixed [0,1] bins make the statistic deterministic and avoid adapting bin edges to
    the current window. Epsilon prevents empty-bin division/log singularities.
    """
    ref = _probabilities(reference, "reference")
    cur = _probabilities(current, "current")
    if bins < 2:
        raise ValueError("bins must be >= 2")
    edges = np.linspace(0.0, 1.0, bins + 1)
    ref_hist = np.histogram(ref, bins=edges)[0].astype(float) / ref.size
    cur_hist = np.histogram(cur, bins=edges)[0].astype(float) / cur.size
    ref_hist = np.clip(ref_hist, epsilon, None)
    cur_hist = np.clip(cur_hist, epsilon, None)
    return float(np.sum((cur_hist - ref_hist) * np.log(cur_hist / ref_hist)))


def probability_drift_report(
    reference: np.ndarray | list[float],
    current: np.ndarray | list[float],
    *,
    no_trade_threshold: float = 0.60,
    psi_limit: float = 0.20,
    mean_shift_limit: float = 0.10,
    abstention_shift_limit: float = 0.15,
    bins: int = 10,
) -> ProbabilityDriftReport:
    """Compare a current prediction window to a frozen validation reference.

    This is monitoring only: it never retrains, promotes, or executes trades. A model
    is flagged when distribution PSI, mean confidence shift, or NO_TRADE-rate shift
    exceeds its configured guardrail.
    """
    ref = _probabilities(reference, "reference")
    cur = _probabilities(current, "current")
    if not 0.5 <= no_trade_threshold <= 1.0:
        raise ValueError("no_trade_threshold must be in [0.5, 1.0]")
    if min(psi_limit, mean_shift_limit, abstention_shift_limit) < 0.0:
        raise ValueError("drift limits must be non-negative")

    psi = population_stability_index(ref, cur, bins=bins)
    mean_shift = abs(float(cur.mean() - ref.mean()))
    ref_trade = (np.maximum(ref, 1.0 - ref) >= no_trade_threshold).mean()
    cur_trade = (np.maximum(cur, 1.0 - cur) >= no_trade_threshold).mean()
    abstention_shift = abs(float((1.0 - cur_trade) - (1.0 - ref_trade)))
    drifted = (
        psi >= psi_limit
        or mean_shift >= mean_shift_limit
        or abstention_shift >= abstention_shift_limit
    )
    return ProbabilityDriftReport(psi, mean_shift, abstention_shift, drifted)
