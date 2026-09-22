import numpy as np
import pytest

from agent6_research.confidence import confidence_bucket_report


def test_bucket_report_accounts_for_every_observation():
    p = np.array([0.0, 0.05, 0.21, 0.49, 0.50, 0.79, 0.99, 1.0])
    y = np.array([0, 0, 0, 1, 0, 1, 1, 1])
    report = confidence_bucket_report(p, y, bins=5)

    assert sum(bucket.count for bucket in report) == len(p)
    assert sum(bucket.coverage for bucket in report) == pytest.approx(1.0)
    assert report[-1].upper == 1.0
    assert any(bucket.mean_probability == pytest.approx(0.995) for bucket in report)


def test_bucket_metrics_are_calculated_only_from_members():
    report = confidence_bucket_report(
        np.array([0.1, 0.2, 0.8, 0.9]),
        np.array([0, 1, 1, 1]),
        bins=2,
    )
    low, high = report
    assert low.count == 2
    assert low.positive_rate == pytest.approx(0.5)
    assert low.brier == pytest.approx(((0.1 - 0.0) ** 2 + (0.2 - 1.0) ** 2) / 2)
    assert high.positive_rate == 1.0


@pytest.mark.parametrize(
    "p,y,bins",
    [
        ([], [], 10),
        ([0.2], [0, 1], 10),
        ([-0.1], [0], 10),
        ([1.1], [1], 10),
        ([np.nan], [1], 10),
        ([0.5], [2], 10),
        ([0.5], [1], 1),
    ],
)
def test_bucket_report_rejects_malformed_inputs(p, y, bins):
    with pytest.raises(ValueError):
        confidence_bucket_report(np.asarray(p), np.asarray(y), bins=bins)
