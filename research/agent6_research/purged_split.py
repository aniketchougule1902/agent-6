"""Purged chronological splits for event-outcome research.

Rows carry an observation time (``ts_ms``) and an outcome horizon end
(``label_end_ms``). Training rows whose outcome horizon overlaps the validation
window are removed, then an optional embargo excludes rows immediately before
validation. This keeps future outcome information out of model fitting.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class PurgedSplit:
    train_indices: np.ndarray
    validation_indices: np.ndarray


def purged_chronological_split(
    ts_ms: np.ndarray,
    label_end_ms: np.ndarray,
    *,
    train_fraction: float = 0.7,
    embargo_rows: int = 0,
) -> PurgedSplit:
    """Return chronological train/validation indices with overlap purge + embargo."""
    ts = np.asarray(ts_ms, dtype=np.int64)
    ends = np.asarray(label_end_ms, dtype=np.int64)
    if ts.ndim != 1 or ends.ndim != 1 or len(ts) != len(ends):
        raise ValueError("ts_ms and label_end_ms must be equal-length 1D arrays")
    if len(ts) < 2:
        raise ValueError("at least two observations are required")
    if not 0.0 < train_fraction < 1.0:
        raise ValueError("train_fraction must be between 0 and 1")
    if embargo_rows < 0:
        raise ValueError("embargo_rows cannot be negative")
    if np.any(np.diff(ts) <= 0):
        raise ValueError("ts_ms must be strictly increasing")
    if np.any(ends < ts):
        raise ValueError("label_end_ms cannot precede ts_ms")

    cut = int(len(ts) * train_fraction)
    cut = min(max(cut, 1), len(ts) - 1)
    validation_indices = np.arange(cut, len(ts), dtype=np.int64)
    validation_start = ts[cut]

    candidate_end = max(0, cut - embargo_rows)
    candidates = np.arange(candidate_end, dtype=np.int64)
    # A training label that resolves at or after validation begins has observed
    # information from the validation era and must be purged.
    train_indices = candidates[ends[candidates] < validation_start]
    if len(train_indices) == 0:
        raise ValueError("purge/embargo removed every training observation")

    return PurgedSplit(train_indices=train_indices, validation_indices=validation_indices)
