import math
import pytest
from agent6_research.critic import build_post_trade_critic


def sample(**overrides):
    args = dict(signal_id="sig-1", model_version="champion-7", symbol="BTCUSDT",
        timeframe="5", regime="trend_up", opened_at_ms=1000, closed_at_ms=2000,
        outcome="sl", entry_probability=0.82, calibrated_probability=0.74,
        net_return_bps=-22.0, mfe_bps=5.0, mae_bps=-20.0, cost_bps=3.0,
        source_hash="sha256:abc")
    args.update(overrides)
    return build_post_trade_critic(**args)


def test_record_is_deterministic_and_content_addressed():
    a, b = sample(), sample()
    assert a == b
    assert a.critic_id == b.critic_id
    assert a.critic_id.startswith("critic_")
    changed = sample(net_return_bps=-23.0)
    assert changed.critic_id != a.critic_id


def test_diagnosis_uses_only_finalized_observations():
    r = sample(calibrated_probability=0.75, entry_probability=0.90, cost_bps=30.0)
    assert "material_calibration_adjustment" in r.diagnosis
    assert "high_confidence_adverse_outcome" in r.diagnosis
    assert "cost_dominated_outcome" in r.diagnosis
    assert "no_rule_based_anomaly" not in r.diagnosis


def test_favorable_excursion_not_captured_is_explicit():
    r = sample(outcome="expired", calibrated_probability=0.55,
               net_return_bps=-1.0, mfe_bps=20.0, mae_bps=-5.0, cost_bps=0.5)
    assert "favorable_excursion_not_captured" in r.diagnosis


@pytest.mark.parametrize("patch", [
    {"signal_id": ""}, {"closed_at_ms": 999}, {"opened_at_ms": 0},
    {"outcome": "active"}, {"entry_probability": 1.01},
    {"calibrated_probability": -0.01}, {"net_return_bps": math.nan},
    {"mfe_bps": -1.0}, {"mae_bps": 1.0}, {"cost_bps": -1.0},
])
def test_invalid_or_nonfinal_inputs_fail_closed(patch):
    with pytest.raises(ValueError):
        sample(**patch)


def test_json_is_stable_and_contains_provenance_not_model_commentary():
    r = sample()
    encoded = r.to_json()
    assert r.critic_id in encoded
    assert "sha256:abc" in encoded
    assert "commentary" not in encoded
