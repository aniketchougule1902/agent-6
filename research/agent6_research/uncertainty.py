"""Deterministic uncertainty/disagreement diagnostics for research-only model ensembles."""

from __future__ import annotations

from dataclasses import dataclass
import math
from typing import Sequence


@dataclass(frozen=True)
class UncertaintyFeatures:
    mean_probability: float
    probability_std: float
    disagreement: float
    predictive_entropy: float
    confidence_margin: float


def ensemble_uncertainty(probabilities: Sequence[float]) -> UncertaintyFeatures:
    """Summarize epistemic disagreement and predictive uncertainty.

    Inputs are per-model probabilities for the same observation. This function is
    intentionally pure and has no fitting step, so it cannot leak future labels.
    """
    if len(probabilities) < 2:
        raise ValueError("at least two model probabilities are required")

    values = [float(p) for p in probabilities]
    if any(not math.isfinite(p) or p < 0.0 or p > 1.0 for p in values):
        raise ValueError("probabilities must be finite and in [0, 1]")

    n = len(values)
    mean = sum(values) / n
    variance = sum((p - mean) ** 2 for p in values) / n
    std = math.sqrt(variance)

    # Mean absolute pairwise disagreement, normalized to [0, 1].
    pair_sum = 0.0
    pairs = 0
    for i in range(n):
        for j in range(i + 1, n):
            pair_sum += abs(values[i] - values[j])
            pairs += 1
    disagreement = pair_sum / pairs

    if mean <= 0.0 or mean >= 1.0:
        entropy = 0.0
    else:
        entropy = -(mean * math.log2(mean) + (1.0 - mean) * math.log2(1.0 - mean))

    margin = abs(mean - 0.5) * 2.0
    return UncertaintyFeatures(
        mean_probability=mean,
        probability_std=std,
        disagreement=disagreement,
        predictive_entropy=entropy,
        confidence_margin=margin,
    )
