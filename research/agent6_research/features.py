"""Leakage-safe research feature factory for Agent-6.

All rolling features are computed from rows at or before ``ts_ms``. Targets belong in a
separate labeling stage and are deliberately rejected as feature inputs.
"""
from __future__ import annotations

from dataclasses import dataclass

import polars as pl

FEATURE_SCHEMA_VERSION = "a6.features.v2"
FORBIDDEN_FEATURE_COLUMNS = {
    "label", "target", "tp_before_sl", "time_to_target_ms", "time_to_stop_ms",
    "mfe_bps", "mae_bps", "future_return",
}


@dataclass(frozen=True)
class FeatureDataset:
    frame: pl.DataFrame
    schema_version: str = FEATURE_SCHEMA_VERSION


def assert_chronological(frame: pl.DataFrame) -> None:
    if "ts_ms" not in frame.columns:
        raise ValueError("feature input requires ts_ms")
    ts = frame.get_column("ts_ms")
    if ts.null_count():
        raise ValueError("ts_ms cannot contain nulls")
    if not ts.is_sorted(descending=False):
        raise ValueError("feature input must be sorted by ts_ms")


def assert_no_target_leakage(frame: pl.DataFrame) -> None:
    leaked = FORBIDDEN_FEATURE_COLUMNS.intersection(frame.columns)
    if leaked:
        raise ValueError(f"target/outcome columns cannot enter feature factory: {sorted(leaked)}")


def _validate_market_inputs(frame: pl.DataFrame) -> None:
    """Reject corrupt observed derivatives values instead of manufacturing a feature."""
    for name in ("price", "bid", "ask", "open_interest", "funding_rate", "mark_price", "index_price"):
        if name not in frame.columns:
            continue
        bad = frame.filter(pl.col(name).is_not_null() & (~pl.col(name).is_finite()))
        if bad.height:
            raise ValueError(f"{name} contains non-finite observations")
    for name in ("price", "bid", "ask", "open_interest", "mark_price", "index_price"):
        if name in frame.columns and frame.filter(pl.col(name).is_not_null() & (pl.col(name) <= 0)).height:
            raise ValueError(f"{name} must be positive when observed")


def build_features(events: pl.DataFrame) -> FeatureDataset:
    """Build causal microstructure, flow and derivatives features.

    Derivatives context is point-in-time only: OI/funding changes use the previous observed
    row, rolling values are trailing-only, and mark/index basis uses values present on that
    same row. Missing observations remain null/neutral rather than being backfilled from the
    future. Outcome/label columns are never accepted here.
    """
    assert_chronological(events)
    assert_no_target_leakage(events)
    _validate_market_inputs(events)

    defaults: dict[str, pl.DataType] = {
        "price": pl.Float64, "bid": pl.Float64, "ask": pl.Float64,
        "signed_notional": pl.Float64, "book_imbalance": pl.Float64,
        "open_interest": pl.Float64, "funding_rate": pl.Float64,
        "liquidation_notional": pl.Float64, "mark_price": pl.Float64,
        "index_price": pl.Float64,
    }
    frame = events
    for name, dtype in defaults.items():
        if name not in frame.columns:
            frame = frame.with_columns(pl.lit(None, dtype=dtype).alias(name))

    frame = frame.with_columns(
        ((pl.col("ask") - pl.col("bid")) / pl.col("price") * 10_000.0).fill_nan(None).alias("spread_bps"),
        pl.col("price").pct_change().alias("return_1"),
        pl.col("signed_notional").fill_null(0.0).rolling_sum(window_size=20).alias("flow_20"),
        pl.col("book_imbalance").fill_null(0.0).rolling_mean(window_size=20).alias("imbalance_20"),
        pl.col("liquidation_notional").fill_null(0.0).rolling_sum(window_size=20).alias("liq_20"),
        pl.col("open_interest").diff().alias("oi_delta_1"),
        pl.col("open_interest").pct_change().alias("oi_return_1"),
        pl.col("funding_rate").diff().alias("funding_delta_1"),
        pl.col("funding_rate").rolling_mean(window_size=20).alias("funding_mean_20"),
        ((pl.col("mark_price") / pl.col("index_price") - 1.0) * 10_000.0).alias("basis_bps"),
    ).with_columns(
        pl.col("return_1").rolling_std(window_size=60).alias("realized_vol_60"),
        pl.col("oi_return_1").rolling_std(window_size=20).alias("oi_vol_20"),
        pl.lit(FEATURE_SCHEMA_VERSION).alias("feature_schema_version"),
    )
    return FeatureDataset(frame=frame)


def chronological_split(dataset: FeatureDataset, train_fraction: float = 0.7, embargo_rows: int = 1) -> tuple[pl.DataFrame, pl.DataFrame]:
    """Chronological train/validation split with an explicit embargo gap."""
    if not 0.0 < train_fraction < 1.0:
        raise ValueError("train_fraction must be between 0 and 1")
    if embargo_rows < 0:
        raise ValueError("embargo_rows cannot be negative")
    frame = dataset.frame
    cut = int(frame.height * train_fraction)
    validation_start = min(frame.height, cut + embargo_rows)
    train = frame.slice(0, cut)
    validation = frame.slice(validation_start)
    if train.height and validation.height and train.get_column("ts_ms").max() >= validation.get_column("ts_ms").min():
        raise AssertionError("chronological split leaked future rows into training")
    return train, validation
