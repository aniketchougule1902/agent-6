"""Research-only CatBoost challenger for leakage-conscious meta-label evaluation."""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from catboost import CatBoostClassifier
from sklearn.metrics import brier_score_loss, log_loss, roc_auc_score


@dataclass(frozen=True)
class CatBoostReport:
    auc: float
    brier: float
    log_loss: float
    train_rows: int
    validation_rows: int


@dataclass(frozen=True)
class CatBoostResult:
    model: CatBoostClassifier
    validation_probability: np.ndarray
    report: CatBoostReport


def train_catboost_challenger(
    features: np.ndarray,
    labels: np.ndarray,
    *,
    validation_fraction: float = 0.2,
    embargo_rows: int = 0,
    random_seed: int = 6,
) -> CatBoostResult:
    """Fit a deterministic chronological CatBoost challenger.

    Rows are never shuffled. The final validation fraction is held out and an
    optional embargo removes rows immediately before validation. This helper is
    deliberately research-only and cannot promote a model or enable execution.
    """
    x = np.asarray(features, dtype=np.float64)
    y = np.asarray(labels)
    if x.ndim != 2 or x.shape[0] < 10 or x.shape[1] == 0:
        raise ValueError("features must be a non-empty 2D matrix with at least 10 rows")
    if y.ndim != 1 or y.shape[0] != x.shape[0]:
        raise ValueError("labels must be a 1D array aligned with features")
    if not np.isfinite(x).all():
        raise ValueError("features contain non-finite values")
    if not np.isin(y, [0, 1]).all():
        raise ValueError("labels must be binary 0/1")
    if not 0.05 <= validation_fraction <= 0.5:
        raise ValueError("validation_fraction must be between 0.05 and 0.5")
    if embargo_rows < 0:
        raise ValueError("embargo_rows must be non-negative")

    validation_rows = max(2, int(np.ceil(x.shape[0] * validation_fraction)))
    validation_start = x.shape[0] - validation_rows
    train_end = validation_start - embargo_rows
    if train_end < 2:
        raise ValueError("embargo leaves too few training rows")

    x_train, y_train = x[:train_end], y[:train_end]
    x_valid, y_valid = x[validation_start:], y[validation_start:]
    if np.unique(y_train).size != 2 or np.unique(y_valid).size != 2:
        raise ValueError("train and validation windows must each contain both classes")

    model = CatBoostClassifier(
        iterations=150,
        depth=6,
        learning_rate=0.05,
        loss_function="Logloss",
        random_seed=random_seed,
        random_strength=0.0,
        bootstrap_type="No",
        allow_writing_files=False,
        verbose=False,
        thread_count=1,
    )
    model.fit(x_train, y_train)
    probability = model.predict_proba(x_valid)[:, 1]

    report = CatBoostReport(
        auc=float(roc_auc_score(y_valid, probability)),
        brier=float(brier_score_loss(y_valid, probability)),
        log_loss=float(log_loss(y_valid, probability, labels=[0, 1])),
        train_rows=train_end,
        validation_rows=validation_rows,
    )
    return CatBoostResult(model=model, validation_probability=probability, report=report)
