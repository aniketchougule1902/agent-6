import numpy as np
import pytest

from agent6_research.drift import population_stability_index, probability_drift_report


def test_identical_probability_windows_have_zero_drift():
    probs = np.linspace(0.05, 0.95, 100)
    report = probability_drift_report(probs, probs)
    assert report.psi == pytest.approx(0.0)
    assert report.mean_shift == pytest.approx(0.0)
    assert report.abstention_shift == pytest.approx(0.0)
    assert report.drifted is False


def test_large_confidence_shift_trips_guardrail():
    reference = np.full(100, 0.52)
    current = np.full(100, 0.90)
    report = probability_drift_report(reference, current, no_trade_threshold=0.60)
    assert report.psi > 0.20
    assert report.mean_shift > 0.10
    assert report.abstention_shift > 0.15
    assert report.drifted is True


def test_psi_is_symmetric_only_in_identity_not_direction():
    reference = np.array([0.1, 0.2, 0.3, 0.4, 0.5] * 20)
    assert population_stability_index(reference, reference) == pytest.approx(0.0)


def test_invalid_probabilities_and_threshold_are_rejected():
    with pytest.raises(ValueError):
        population_stability_index([0.1, 1.1], [0.2, 0.3])
    with pytest.raises(ValueError):
        probability_drift_report([0.1], [0.2], no_trade_threshold=0.49)
    with pytest.raises(ValueError):
        population_stability_index([0.1], [0.2], bins=1)
