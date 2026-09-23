import numpy as np
import pytest

from agent6_research.regimes import causal_regime_labels


def test_regime_labels_are_prefix_invariant():
    prefix = np.exp(np.linspace(np.log(100.0), np.log(120.0), 80))
    full = np.concatenate([prefix, np.array([80.0, 140.0, 70.0, 150.0])])
    prefix_labels = causal_regime_labels(prefix, lookback=10)
    full_labels = causal_regime_labels(full, lookback=10)
    assert np.array_equal(prefix_labels, full_labels[: prefix.size])


def test_up_and_down_trends_are_detected():
    up = np.exp(np.linspace(np.log(100.0), np.log(150.0), 80))
    down = np.exp(np.linspace(np.log(150.0), np.log(100.0), 80))
    assert causal_regime_labels(up, lookback=10)[-1] == "trend_up"
    assert causal_regime_labels(down, lookback=10)[-1] == "trend_down"


def test_invalid_inputs_fail_closed():
    with pytest.raises(ValueError):
        causal_regime_labels(np.array([100.0, np.nan, 101.0]))
    with pytest.raises(ValueError):
        causal_regime_labels(np.array([100.0, 0.0, 101.0]))
    with pytest.raises(ValueError):
        causal_regime_labels(np.array([100.0, 101.0]), lookback=2)
