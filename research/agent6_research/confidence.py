"""Calibration diagnostics by confidence bucket.

Research-only reporting: this module never enables execution or changes a model.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class ConfidenceBucket:
    lower: float
    upper: float
    count: int
    coverage: float
    mean_probability: float
    positive_rate: float
    brier: float


def confidence_bucket_report(
    probabilities: np.ndarray,
    labels: np.ndarray,
    *,
    bins: int = 10,
) -> list[ConfidenceBucket]:
    """Return deterministic equal-width probability-bucket diagnostics.

    Empty buckets are omitted. Bins are [lower, upper), except the final bucket
    includes probability 1.0. This is descriptive held-out evaluation only;
    callers must not use the report to tune on test data.
    """
    p = np.asarray(probabilities, dtype=float).reshape(-1)
    y = np.asarray(labels, dtype=float).reshape(-1)
    if p.size == 0 or p.size != y.size:
        raise ValueError("probabilities and labels must be non-empty and equal length")
    if bins < 2:
        raise ValueError("bins must be >= 2")
    if not np.all(np.isfinite(p)) or np.any((p < 0.0) | (p > 1.0)):
        raise ValueError("probabilities must be finite values in [0, 1]")
    if not np.all(np.isfinite(y)) or not np.all(np.isin(y, [0.0, 1.0])):
        raise ValueError("labels must be binary finite values")

    edges = np.linspace(0.0, 1.0, bins + 1)
    # right=False gives [edge_i, edge_i+1); clip keeps p==1 in the final bin.
    bucket_ids = np.clip(np.digitize(p, edges[1:-1], right=False), 0, bins - 1)
    report: list[ConfidenceBucket] = []
    total = float(p.size)
    for idx in range(bins):
        mask = bucket_ids == idx
        count = int(mask.sum())
        if count == 0:
            continue
        bp = p[mask]
        by = y[mask]
        report.append(
            ConfidenceBucket(
                lower=float(edges[idx]),
                upper=float(edges[idx + 1]),
                count=count,
                coverage=count / total,
                mean_probability=float(bp.mean()),
                positive_rate=float(by.mean()),
                brier=float(np.mean((bp - by) ** 2)),
            )
        )
    return report
