from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import numpy as np
from sklearn.isotonic import IsotonicRegression
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import brier_score_loss


@dataclass(frozen=True)
class CalibrationReport:
    brier: float
    ece: float
    rows: int


@dataclass
class ProbabilityCalibrator:
    method: Literal["platt", "isotonic"]
    model: object
    report: CalibrationReport

    def transform(self, probability: np.ndarray) -> np.ndarray:
        p = _probability_vector(probability)
        if self.method == "platt":
            out = self.model.predict_proba(_logit(p).reshape(-1, 1))[:, 1]
        else:
            out = self.model.predict(p)
        return np.clip(np.asarray(out, dtype=np.float64), 0.0, 1.0)


@dataclass(frozen=True)
class AbstentionPolicy:
    threshold: float
    min_coverage: float
    validation_utility: float

    def trade_mask(self, probability: np.ndarray) -> np.ndarray:
        p = _probability_vector(probability)
        return p >= self.threshold


def fit_calibrator(
    probability: np.ndarray,
    labels: np.ndarray,
    *,
    method: Literal["platt", "isotonic"] = "platt",
    ece_bins: int = 10,
) -> ProbabilityCalibrator:
    """Fit calibration on a held-out chronological calibration partition only.

    Callers are responsible for passing predictions that were not used to fit the
    underlying classifier. This function never refits or inspects feature data.
    """
    p = _probability_vector(probability)
    y = _binary_labels(labels, len(p))
    if len(p) < 20 or len(np.unique(y)) < 2:
        raise ValueError("calibration requires at least 20 rows containing both classes")

    if method == "platt":
        model = LogisticRegression(random_state=6, solver="lbfgs")
        model.fit(_logit(p).reshape(-1, 1), y)
        calibrated = model.predict_proba(_logit(p).reshape(-1, 1))[:, 1]
    elif method == "isotonic":
        model = IsotonicRegression(out_of_bounds="clip")
        model.fit(p, y)
        calibrated = model.predict(p)
    else:
        raise ValueError("method must be 'platt' or 'isotonic'")

    calibrated = np.clip(np.asarray(calibrated, dtype=np.float64), 0.0, 1.0)
    report = CalibrationReport(
        brier=float(brier_score_loss(y, calibrated)),
        ece=expected_calibration_error(calibrated, y, bins=ece_bins),
        rows=len(y),
    )
    return ProbabilityCalibrator(method=method, model=model, report=report)


def expected_calibration_error(
    probability: np.ndarray, labels: np.ndarray, *, bins: int = 10
) -> float:
    p = _probability_vector(probability)
    y = _binary_labels(labels, len(p))
    if bins < 2 or bins > 100:
        raise ValueError("bins must be between 2 and 100")

    edges = np.linspace(0.0, 1.0, bins + 1)
    total = float(len(p))
    ece = 0.0
    for index in range(bins):
        left, right = edges[index], edges[index + 1]
        mask = (p >= left) & (p <= right if index == bins - 1 else p < right)
        count = int(mask.sum())
        if count:
            ece += count / total * abs(float(p[mask].mean()) - float(y[mask].mean()))
    return float(ece)


def select_no_trade_threshold(
    probability: np.ndarray,
    labels: np.ndarray,
    *,
    win_value: float = 1.0,
    loss_value: float = -1.0,
    min_coverage: float = 0.1,
) -> AbstentionPolicy:
    """Choose a validation-only NO_TRADE threshold by realized utility.

    The objective is mean utility across *all* validation opportunities, so
    abstaining is worth zero and tiny high-confidence subsets cannot win merely by
    having a high conditional win rate. Ties prefer the higher threshold.
    """
    p = _probability_vector(probability)
    y = _binary_labels(labels, len(p))
    if not 0.0 < min_coverage <= 1.0:
        raise ValueError("min_coverage must be in (0, 1]")
    if win_value <= 0.0 or loss_value >= 0.0:
        raise ValueError("win_value must be positive and loss_value negative")

    candidates = np.unique(np.concatenate(([0.0, 1.0], p)))
    best: tuple[float, float, float] | None = None
    for threshold in candidates:
        mask = p >= threshold
        coverage = float(mask.mean())
        if coverage + 1e-12 < min_coverage:
            continue
        realized = np.where(y[mask] == 1, win_value, loss_value).sum() / len(y)
        candidate = (float(realized), float(threshold), coverage)
        if best is None or candidate[:2] > best[:2]:
            best = candidate

    if best is None:
        raise ValueError("no threshold satisfies min_coverage")
    utility, threshold, coverage = best
    return AbstentionPolicy(threshold=threshold, min_coverage=coverage, validation_utility=utility)


def _probability_vector(probability: np.ndarray) -> np.ndarray:
    p = np.asarray(probability, dtype=np.float64)
    if p.ndim != 1 or len(p) == 0 or not np.isfinite(p).all():
        raise ValueError("probabilities must be a non-empty finite vector")
    if ((p < 0.0) | (p > 1.0)).any():
        raise ValueError("probabilities must be in [0, 1]")
    return p


def _binary_labels(labels: np.ndarray, rows: int) -> np.ndarray:
    y = np.asarray(labels)
    if y.ndim != 1 or len(y) != rows or not np.isin(y, [0, 1]).all():
        raise ValueError("labels must be a matching binary vector")
    return y.astype(np.int8, copy=False)


def _logit(probability: np.ndarray) -> np.ndarray:
    p = np.clip(probability, 1e-6, 1.0 - 1e-6)
    return np.log(p / (1.0 - p))
