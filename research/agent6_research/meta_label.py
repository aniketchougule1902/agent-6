from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from lightgbm import LGBMClassifier
from sklearn.metrics import brier_score_loss, log_loss, roc_auc_score


@dataclass(frozen=True)
class MetaLabelMetrics:
    auc: float
    brier: float
    log_loss: float
    train_rows: int
    validation_rows: int


@dataclass
class MetaLabelBaseline:
    model: LGBMClassifier
    feature_names: tuple[str, ...]
    metrics: MetaLabelMetrics

    def predict_proba(self, features: np.ndarray) -> np.ndarray:
        matrix = _validate_matrix(features, len(self.feature_names))
        return self.model.predict_proba(matrix)[:, 1]


def fit_lightgbm_meta_label(
    features: np.ndarray,
    labels: np.ndarray,
    feature_names: list[str] | tuple[str, ...],
    *,
    validation_fraction: float = 0.2,
    embargo_rows: int = 0,
    random_state: int = 6,
) -> MetaLabelBaseline:
    """Fit a chronological, embargoed TP-before-SL meta-label baseline.

    Rows must already be ordered by event time. The function never shuffles and
    rejects non-binary/non-finite targets so research failures cannot silently
    leak into model evaluation.
    """
    names = tuple(feature_names)
    x = _validate_matrix(features, len(names))
    y = np.asarray(labels)
    if y.ndim != 1 or len(y) != len(x):
        raise ValueError("labels must be one-dimensional and match feature rows")
    if not np.isin(y, [0, 1]).all():
        raise ValueError("meta labels must be binary TP-before-SL outcomes")
    if not 0.05 <= validation_fraction <= 0.5:
        raise ValueError("validation_fraction must be between 0.05 and 0.5")
    if embargo_rows < 0:
        raise ValueError("embargo_rows cannot be negative")

    validation_rows = max(1, int(np.ceil(len(x) * validation_fraction)))
    validation_start = len(x) - validation_rows
    train_end = validation_start - embargo_rows
    if train_end < 20 or validation_rows < 2:
        raise ValueError("not enough chronological rows after validation split and embargo")

    x_train, y_train = x[:train_end], y[:train_end]
    x_valid, y_valid = x[validation_start:], y[validation_start:]
    if len(np.unique(y_train)) < 2 or len(np.unique(y_valid)) < 2:
        raise ValueError("both chronological partitions must contain both classes")

    model = LGBMClassifier(
        n_estimators=200,
        learning_rate=0.03,
        num_leaves=15,
        max_depth=5,
        subsample=0.9,
        colsample_bytree=0.9,
        reg_lambda=1.0,
        random_state=random_state,
        n_jobs=1,
        verbosity=-1,
    )
    model.fit(x_train, y_train)
    probability = np.clip(model.predict_proba(x_valid)[:, 1], 1e-6, 1 - 1e-6)
    metrics = MetaLabelMetrics(
        auc=float(roc_auc_score(y_valid, probability)),
        brier=float(brier_score_loss(y_valid, probability)),
        log_loss=float(log_loss(y_valid, probability, labels=[0, 1])),
        train_rows=len(x_train),
        validation_rows=len(x_valid),
    )
    return MetaLabelBaseline(model=model, feature_names=names, metrics=metrics)


def _validate_matrix(features: np.ndarray, expected_columns: int) -> np.ndarray:
    matrix = np.asarray(features, dtype=np.float64)
    if matrix.ndim != 2 or matrix.shape[1] != expected_columns:
        raise ValueError("feature matrix shape does not match feature schema")
    if len(matrix) == 0 or not np.isfinite(matrix).all():
        raise ValueError("feature matrix must be non-empty and finite")
    return matrix
