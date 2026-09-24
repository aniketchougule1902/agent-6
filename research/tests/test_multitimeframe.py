import polars as pl
import pytest

from agent6_research.multitimeframe import add_completed_timeframe_context


def rows(n: int = 20) -> pl.DataFrame:
    # One observation per minute, deliberately starting inside the first 3m bucket.
    return pl.DataFrame({"ts_ms": [60_000 * i for i in range(1, n + 1)], "price": [100.0 + i for i in range(n)]})


def test_incomplete_higher_timeframe_bucket_is_never_visible_early():
    out = add_completed_timeframe_context(rows(8), base_interval_ms=60_000, timeframe_multiples=(3,))
    # 3m bucket ending at 180s is first visible at t=180s; it has no prior completed bar.
    assert out[0, "mtf_3x_return_1"] is None
    assert out[1, "mtf_3x_return_1"] is None
    # Second completed 3m close becomes visible at t=360s, never at 300s.
    assert out[4, "mtf_3x_return_1"] is None
    assert out[5, "mtf_3x_return_1"] is not None


def test_prefix_invariance_blocks_future_data_leakage():
    base = rows(18)
    prefix = base.head(12)
    a = add_completed_timeframe_context(prefix, base_interval_ms=60_000)
    changed = base.with_columns(
        pl.when(pl.col("ts_ms") > prefix["ts_ms"][-1])
        .then(pl.lit(9_999_999.0))
        .otherwise(pl.col("price"))
        .alias("price")
    )
    b = add_completed_timeframe_context(changed, base_interval_ms=60_000).head(prefix.height)
    for name in [c for c in a.columns if c.startswith("mtf_")]:
        assert a[name].to_list() == b[name].to_list()


def test_exact_boundary_only_uses_bar_that_has_just_completed():
    frame = pl.DataFrame({"ts_ms": [0, 60_000, 120_000, 179_999, 180_000, 240_000, 300_000, 360_000], "price": [100.,101.,102.,103.,104.,105.,106.,107.]})
    out = add_completed_timeframe_context(frame, base_interval_ms=60_000, timeframe_multiples=(3,))
    # The first bucket's close is 103 at 179999 and is eligible at 180000.
    # A return still needs a second completed bucket, which is eligible at 360000.
    assert out[4, "mtf_3x_return_1"] is None
    assert out[6, "mtf_3x_return_1"] is None
    assert out[7, "mtf_3x_return_1"] == pytest.approx(106.0 / 103.0 - 1.0)


def test_invalid_inputs_fail_closed():
    with pytest.raises(ValueError):
        add_completed_timeframe_context(rows(), base_interval_ms=0)
    with pytest.raises(ValueError):
        add_completed_timeframe_context(rows(), base_interval_ms=60_000, timeframe_multiples=(1,))
    with pytest.raises(ValueError):
        add_completed_timeframe_context(rows().reverse(), base_interval_ms=60_000)
    bad = rows().with_columns(pl.when(pl.col("ts_ms") == 60_000).then(float("nan")).otherwise(pl.col("price")).alias("price"))
    with pytest.raises(ValueError):
        add_completed_timeframe_context(bad, base_interval_ms=60_000)
