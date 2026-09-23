import numpy as np
import pytest

from agent6_research.catboost_challenger import train_catboost_challenger


def dataset(rows: int = 80):
    t = np.arange(rows, dtype=float)
    x = np.column_stack((np.sin(t / 4), np.cos(t / 7), (t % 9) / 9))
    y = ((t.astype(int) % 4) < 2).astype(int)
    return x, y


def test_catboost_challenger_is_chronological_and_probabilistic():
    x, y = dataset()
    result = train_catboost_challenger(x, y, validation_fraction=0.25, embargo_rows=3)
    assert result.report.validation_rows == 20
    assert result.report.train_rows == 57
    assert result.validation_probability.shape == (20,)
    assert np.all((result.validation_probability >= 0) & (result.validation_probability <= 1))
    assert 0 <= result.report.brier <= 1


def test_catboost_challenger_is_deterministic():
    x, y = dataset()
    first = train_catboost_challenger(x, y, random_seed=6)
    second = train_catboost_challenger(x, y, random_seed=6)
    np.testing.assert_allclose(first.validation_probability, second.validation_probability)


def test_catboost_challenger_rejects_nonfinite_and_bad_labels():
    x, y = dataset()
    broken = x.copy()
    broken[3, 0] = np.nan
    with pytest.raises(ValueError, match="non-finite"):
        train_catboost_challenger(broken, y)

    bad_y = y.copy()
    bad_y[2] = 2
    with pytest.raises(ValueError, match="binary"):
        train_catboost_challenger(x, bad_y)
