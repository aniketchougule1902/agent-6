from __future__ import annotations

from dataclasses import dataclass

import numpy as np
import optuna
from lightgbm import LGBMClassifier
from sklearn.metrics import brier_score_loss, log_loss, roc_auc_score


@dataclass(frozen=True)
class TuningResult:
    best_params: dict[str, float | int]
    inner_best_brier: float
    outer_auc: float
    outer_brier: float
    outer_log_loss: float
    development_rows: int
    outer_rows: int
    trials: int


def tune_lightgbm_challenger(
    features: np.ndarray,
    labels: np.ndarray,
    *,
    outer_fraction: float = 0.20,
    inner_fraction: float = 0.20,
    embargo_rows: int = 0,
    trials: int = 12,
    seed: int = 6,
) -> TuningResult:
    """Bounded research-only tuning with an untouched chronological outer holdout.

    Hyperparameters are selected only on an inner validation tail of the
    development prefix. The final chronological outer tail is never passed to
    Optuna and is evaluated exactly once after parameter selection. Rows must
    already be ordered by event time. This function does not promote artifacts.
    """
    x = np.asarray(features, dtype=np.float64)
    y = np.asarray(labels)
    if x.ndim != 2 or len(x) == 0 or not np.isfinite(x).all():
        raise ValueError("features must be a non-empty finite 2D matrix")
    if y.ndim != 1 or len(y) != len(x) or not np.isin(y, [0, 1]).all():
        raise ValueError("labels must be binary and match feature rows")
    if not 0.10 <= outer_fraction <= 0.40 or not 0.10 <= inner_fraction <= 0.40:
        raise ValueError("inner/outer fractions must be between 0.10 and 0.40")
    if embargo_rows < 0:
        raise ValueError("embargo_rows cannot be negative")
    if not 1 <= trials <= 50:
        raise ValueError("trials must be bounded between 1 and 50")

    outer_rows = max(2, int(np.ceil(len(x) * outer_fraction)))
    outer_start = len(x) - outer_rows
    development_end = outer_start - embargo_rows
    if development_end < 40:
        raise ValueError("not enough development rows after outer holdout and embargo")
    x_dev, y_dev = x[:development_end], y[:development_end]
    x_outer, y_outer = x[outer_start:], y[outer_start:]

    inner_rows = max(2, int(np.ceil(len(x_dev) * inner_fraction)))
    inner_start = len(x_dev) - inner_rows
    inner_train_end = inner_start - embargo_rows
    if inner_train_end < 20:
        raise ValueError("not enough inner-training rows after embargo")
    x_train, y_train = x_dev[:inner_train_end], y_dev[:inner_train_end]
    x_inner, y_inner = x_dev[inner_start:], y_dev[inner_start:]
    for name, part in (("inner train", y_train), ("inner validation", y_inner), ("outer holdout", y_outer)):
        if len(np.unique(part)) < 2:
            raise ValueError(f"{name} must contain both classes")

    sampler = optuna.samplers.TPESampler(seed=seed)
    study = optuna.create_study(direction="minimize", sampler=sampler)

    def objective(trial: optuna.Trial) -> float:
        params = {
            "n_estimators": trial.suggest_int("n_estimators", 80, 320, step=40),
            "learning_rate": trial.suggest_float("learning_rate", 0.01, 0.08, log=True),
            "num_leaves": trial.suggest_int("num_leaves", 7, 31, step=4),
            "max_depth": trial.suggest_int("max_depth", 3, 7),
            "min_child_samples": trial.suggest_int("min_child_samples", 10, 60, step=10),
            "reg_lambda": trial.suggest_float("reg_lambda", 0.1, 5.0, log=True),
        }
        model = _model(params, seed)
        model.fit(x_train, y_train)
        probability = np.clip(model.predict_proba(x_inner)[:, 1], 1e-6, 1 - 1e-6)
        return float(brier_score_loss(y_inner, probability))

    study.optimize(objective, n_trials=trials, show_progress_bar=False)
    best = dict(study.best_params)
    final = _model(best, seed)
    final.fit(x_dev, y_dev)
    probability = np.clip(final.predict_proba(x_outer)[:, 1], 1e-6, 1 - 1e-6)
    return TuningResult(
        best_params=best,
        inner_best_brier=float(study.best_value),
        outer_auc=float(roc_auc_score(y_outer, probability)),
        outer_brier=float(brier_score_loss(y_outer, probability)),
        outer_log_loss=float(log_loss(y_outer, probability, labels=[0, 1])),
        development_rows=len(x_dev),
        outer_rows=len(x_outer),
        trials=trials,
    )


def _model(params: dict[str, float | int], seed: int) -> LGBMClassifier:
    return LGBMClassifier(
        **params,
        subsample=0.9,
        colsample_bytree=0.9,
        random_state=seed,
        n_jobs=1,
        verbosity=-1,
    )
