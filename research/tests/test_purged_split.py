import numpy as np
import pytest

from agent6_research.purged_split import purged_chronological_split


def test_purges_overlapping_outcomes_and_embargoes_boundary_rows():
    ts = np.arange(10, dtype=np.int64) * 100
    ends = ts + 10
    # Row 5 starts in training but its outcome resolves inside validation.
    ends[5] = 750

    split = purged_chronological_split(ts, ends, train_fraction=0.7, embargo_rows=1)

    assert split.validation_indices.tolist() == [7, 8, 9]
    # Row 6 is embargoed; row 5 is purged because label_end >= validation start.
    assert split.train_indices.tolist() == [0, 1, 2, 3, 4]
    assert np.all(ends[split.train_indices] < ts[split.validation_indices[0]])


def test_rejects_non_chronological_or_impossible_horizons():
    with pytest.raises(ValueError, match="strictly increasing"):
        purged_chronological_split(np.array([1, 1, 2]), np.array([1, 2, 2]))
    with pytest.raises(ValueError, match="cannot precede"):
        purged_chronological_split(np.array([1, 2, 3]), np.array([1, 1, 3]))


def test_fails_closed_if_purge_removes_all_training_rows():
    ts = np.array([100, 200, 300, 400])
    ends = np.array([500, 500, 500, 500])
    with pytest.raises(ValueError, match="removed every training"):
        purged_chronological_split(ts, ends, train_fraction=0.5)
