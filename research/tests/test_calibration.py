import numpy as np
import pytest

from agent6_research.calibration import (
    expected_calibration_error,
    fit_calibrator,
    select_no_trade_threshold,
)


def test_platt_calibration_is_bounded_and_reported():
    probability = np.linspace(0.05, 0.95, 40)
    labels = (probability > 0.55).astype(int)
    calibrator = fit_calibrator(probability, labels, method="platt", ece_bins=5)
    calibrated = calibrator.transform(np.array([0.1, 0.5, 0.9]))
    assert np.all((calibrated >= 0.0) & (calibrated <= 1.0))
    assert calibrator.report.rows == 40
    assert 0.0 <= calibrator.report.brier <= 1.0
    assert 0.0 <= calibrator.report.ece <= 1.0


def test_isotonic_transform_is_monotonic():
    probability = np.linspace(0.01, 0.99, 50)
    labels = np.array(([0] * 15) + ([0, 1] * 10) + ([1] * 15))
    calibrator = fit_calibrator(probability, labels, method="isotonic")
    transformed = calibrator.transform(probability)
    assert np.all(np.diff(transformed) >= -1e-12)


def test_ece_is_zero_for_perfect_binary_probabilities():
    probability = np.array([0.0, 1.0, 0.0, 1.0])
    labels = np.array([0, 1, 0, 1])
    assert expected_calibration_error(probability, labels, bins=4) == pytest.approx(0.0)


def test_no_trade_threshold_prefers_profitable_confident_subset():
    probability = np.array([0.95, 0.90, 0.85, 0.80, 0.40, 0.35, 0.30, 0.20])
    labels = np.array([1, 1, 1, 0, 0, 0, 1, 0])
    policy = select_no_trade_threshold(probability, labels, min_coverage=0.25)
    mask = policy.trade_mask(probability)
    assert policy.threshold >= 0.85
    assert mask.sum() >= 2
    assert policy.validation_utility > 0.0


def test_validation_rejects_invalid_inputs():
    with pytest.raises(ValueError):
        fit_calibrator(np.array([0.2, 1.2] * 10), np.array([0, 1] * 10))
    with pytest.raises(ValueError):
        select_no_trade_threshold(np.array([0.2, 0.8]), np.array([0, 2]))
    with pytest.raises(ValueError):
        expected_calibration_error(np.array([0.2, 0.8]), np.array([0, 1]), bins=1)
