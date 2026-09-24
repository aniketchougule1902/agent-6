import polars as pl
import pytest

from agent6_research.features import FEATURE_SCHEMA_VERSION, build_features, chronological_split


def rows(n: int = 100) -> pl.DataFrame:
    return pl.DataFrame({
        "ts_ms": list(range(1, n + 1)),
        "price": [100.0 + i for i in range(n)],
        "bid": [99.5 + i for i in range(n)],
        "ask": [100.5 + i for i in range(n)],
        "signed_notional": [1.0] * n,
        "book_imbalance": [0.1] * n,
        "open_interest": [1000.0 + i for i in range(n)],
        "funding_rate": [0.0001 + i * 0.000001 for i in range(n)],
        "liquidation_notional": [0.0] * n,
        "mark_price": [100.05 + i for i in range(n)],
        "index_price": [100.0 + i for i in range(n)],
    })


def test_rejects_unsorted_time() -> None:
    with pytest.raises(ValueError, match="sorted"):
        build_features(rows(3).reverse())


def test_rejects_outcome_columns() -> None:
    with pytest.raises(ValueError, match="target/outcome"):
        build_features(rows(3).with_columns(pl.lit(1).alias("tp_before_sl")))


def test_prefix_invariance_proves_no_future_row_dependency() -> None:
    full = build_features(rows(100)).frame
    prefix = build_features(rows(70)).frame
    comparable = [c for c in prefix.columns if c != "feature_schema_version"]
    assert full.head(70).select(comparable).equals(prefix.select(comparable), null_equal=True)


def test_derivatives_features_are_point_in_time_and_versioned() -> None:
    base = rows(80)
    full = build_features(base).frame
    mutated = base.with_columns(
        pl.when(pl.col("ts_ms") > 60).then(pl.lit(9_999_999.0)).otherwise(pl.col("open_interest")).alias("open_interest"),
        pl.when(pl.col("ts_ms") > 60).then(pl.lit(0.05)).otherwise(pl.col("funding_rate")).alias("funding_rate"),
        pl.when(pl.col("ts_ms") > 60).then(pl.lit(500.0)).otherwise(pl.col("mark_price")).alias("mark_price"),
    )
    changed = build_features(mutated).frame
    cols = ["oi_delta_1", "oi_return_1", "oi_vol_20", "funding_delta_1", "funding_mean_20", "basis_bps"]
    assert full.head(60).select(cols).equals(changed.head(60).select(cols), null_equal=True)
    assert full["feature_schema_version"][0] == FEATURE_SCHEMA_VERSION == "a6.features.v2"


def test_basis_and_derivatives_changes_have_expected_direction() -> None:
    out = build_features(rows(4)).frame
    assert out["oi_delta_1"][1] == pytest.approx(1.0)
    assert out["oi_return_1"][1] > 0
    assert out["funding_delta_1"][1] > 0
    assert out["basis_bps"][0] == pytest.approx(5.0)


def test_corrupt_derivatives_observations_fail_closed() -> None:
    with pytest.raises(ValueError, match="open_interest must be positive"):
        build_features(rows(4).with_columns(pl.when(pl.col("ts_ms") == 3).then(pl.lit(0.0)).otherwise(pl.col("open_interest")).alias("open_interest")))
    with pytest.raises(ValueError, match="funding_rate contains non-finite"):
        build_features(rows(4).with_columns(pl.when(pl.col("ts_ms") == 3).then(pl.lit(float("nan"))).otherwise(pl.col("funding_rate")).alias("funding_rate")))
    with pytest.raises(ValueError, match="index_price must be positive"):
        build_features(rows(4).with_columns(pl.lit(0.0).alias("index_price")))


def test_missing_derivatives_inputs_do_not_backfill_future_values() -> None:
    sparse = rows(5).drop(["mark_price", "index_price", "funding_rate"])
    out = build_features(sparse).frame
    assert out["basis_bps"].null_count() == 5
    assert out["funding_delta_1"].null_count() == 5


def test_chronological_split_has_embargo() -> None:
    dataset = build_features(rows(100))
    train, validation = chronological_split(dataset, train_fraction=0.7, embargo_rows=5)
    assert train.height == 70
    assert validation.height == 25
    assert train["ts_ms"].max() < validation["ts_ms"].min()
    assert validation["ts_ms"].min() == 76
