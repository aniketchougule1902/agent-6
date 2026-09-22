import numpy as np
import pytest

from agent6_research.meta_label import fit_lightgbm_meta_label


def _dataset(rows: int = 120):
    t = np.arange(rows, dtype=float)
    x = np.column_stack((np.sin(t / 5), np.cos(t / 7), (t % 9) / 9))
    y = ((t.astype(int) % 4) < 2).astype(int)
    return x, y


def test_meta_label_uses_chronological_embargoed_split():
    x, y = _dataset()
    baseline = fit_lightgbm_meta_label(
        x, y, ["flow", "microprice", "oi_delta"], validation_fraction=0.2, embargo_rows=5
    )
    assert baseline.metrics.validation_rows == 24
    assert baseline.metrics.train_rows == 91
    assert 0.0 <= baseline.metrics.auc <= 1.0
    assert 0.0 <= baseline.metrics.brier <= 1.0
    probability = baseline.predict_proba(x[-3:])
    assert probability.shape == (3,)
    assert np.all((probability >= 0.0) & (probability <= 1.0))


def test_meta_label_rejects_non_binary_outcomes():
    x, y = _dataset()
    y[3] = 2
    with pytest.raises(ValueError, match="binary"):
        fit_lightgbm_meta_label(x, y, ["a", "b", "c"])


def test_meta_label_rejects_schema_mismatch():
    x, y = _dataset()
    with pytest.raises(ValueError, match="schema"):
        fit_lightgbm_meta_label(x, y, ["a", "b"])
