import math

import pytest

from agent6_research.uncertainty import ensemble_uncertainty


def test_identical_models_have_zero_disagreement():
    result = ensemble_uncertainty([0.8, 0.8, 0.8])
    assert result.mean_probability == pytest.approx(0.8)
    assert result.probability_std == pytest.approx(0.0)
    assert result.disagreement == pytest.approx(0.0)
    assert result.confidence_margin == pytest.approx(0.6)


def test_opposed_models_surface_maximum_binary_uncertainty():
    result = ensemble_uncertainty([0.0, 1.0])
    assert result.mean_probability == pytest.approx(0.5)
    assert result.disagreement == pytest.approx(1.0)
    assert result.predictive_entropy == pytest.approx(1.0)
    assert result.confidence_margin == pytest.approx(0.0)


def test_entropy_collapses_at_certain_consensus():
    result = ensemble_uncertainty([1.0, 1.0])
    assert result.predictive_entropy == pytest.approx(0.0)
    assert result.confidence_margin == pytest.approx(1.0)


@pytest.mark.parametrize(
    "values",
    [[], [0.5], [-0.1, 0.5], [0.5, 1.1], [math.nan, 0.5], [math.inf, 0.5]],
)
def test_invalid_inputs_fail_closed(values):
    with pytest.raises(ValueError):
        ensemble_uncertainty(values)
