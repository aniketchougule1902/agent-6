import numpy as np
import pytest

from agent6_research.tuning import tune_lightgbm_challenger


def dataset(rows: int = 160):
    rng = np.random.default_rng(6)
    x = rng.normal(size=(rows, 4))
    # Alternating labels guarantee both classes in every chronological tail;
    # weak causal structure keeps this a plumbing test, not performance evidence.
    y = (np.arange(rows) % 2).astype(int)
    x[:, 0] += y * 0.35
    return x, y


def test_bounded_tuning_reports_inner_and_untouched_outer_metrics():
    x, y = dataset()
    result = tune_lightgbm_challenger(
        x, y, outer_fraction=0.20, inner_fraction=0.20, embargo_rows=2, trials=2
    )
    assert result.trials == 2
    assert result.outer_rows == 32
    assert result.development_rows == 126
    assert 0.0 <= result.outer_auc <= 1.0
    assert 0.0 <= result.outer_brier <= 1.0
    assert result.outer_log_loss >= 0.0
    assert set(result.best_params) == {
        "n_estimators", "learning_rate", "num_leaves", "max_depth",
        "min_child_samples", "reg_lambda",
    }


def test_future_outer_feature_mutation_cannot_change_selected_hyperparameters():
    x, y = dataset()
    first = tune_lightgbm_challenger(x, y, embargo_rows=3, trials=3, seed=11)
    changed = x.copy()
    outer_rows = int(np.ceil(len(x) * 0.20))
    changed[-outer_rows:] = changed[-outer_rows:] * 1000.0 + 777.0
    second = tune_lightgbm_challenger(changed, y, embargo_rows=3, trials=3, seed=11)
    assert first.best_params == second.best_params
    assert first.inner_best_brier == pytest.approx(second.inner_best_brier)
    # Outer metrics are allowed to change: the holdout is evaluation-only.


def test_tuning_fails_closed_on_unsafe_or_unbounded_requests():
    x, y = dataset()
    with pytest.raises(ValueError, match="bounded"):
        tune_lightgbm_challenger(x, y, trials=51)
    bad = x.copy()
    bad[5, 0] = np.nan
    with pytest.raises(ValueError, match="finite"):
        tune_lightgbm_challenger(bad, y)
    one_class = np.zeros_like(y)
    with pytest.raises(ValueError, match="both classes"):
        tune_lightgbm_challenger(x, one_class, trials=1)
