import polars as pl
import pytest

from agent6_research.features import build_features, chronological_split


def rows(n: int = 100) -> pl.DataFrame:
    return pl.DataFrame({
        "ts_ms": list(range(1, n + 1)),
        "price": [100.0 + i for i in range(n)],
        "bid": [99.5 + i for i in range(n)],
        "ask": [100.5 + i for i in range(n)],
        "signed_notional": [1.0] * n,
        "book_imbalance": [0.1] * n,
        "open_interest": [1000.0 + i for i in range(n)],
        "funding_rate": [0.0001] * n,
        "liquidation_notional": [0.0] * n,
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


def test_chronological_split_has_embargo() -> None:
    dataset = build_features(rows(100))
    train, validation = chronological_split(dataset, train_fraction=0.7, embargo_rows=5)
    assert train.height == 70
    assert validation.height == 25
    assert train["ts_ms"].max() < validation["ts_ms"].min()
    assert validation["ts_ms"].min() == 76
