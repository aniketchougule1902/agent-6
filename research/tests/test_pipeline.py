import json
from pathlib import Path
import sys

import numpy as np
import pytest

from agent6_research.artifacts import verify_artifact

# Import from scripts/train_model
REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from train_model import (
    FEATURE_NAMES,
    compute_features_and_labels,
    export_model_artifact,
    train_champion_model,
)


def generate_synthetic_candles(num_bars: int = 1000, seed: int = 42) -> list[dict]:
    """Deterministic test fixture only; production training never falls back to synthetic data."""
    rng = np.random.default_rng(seed)
    start_ms = 1_700_000_000_000
    price = 65_000.0
    candles = []
    for i in range(num_bars):
        vol = rng.uniform(0.0005, 0.0025)
        regime = np.sin(i / 100.0) * 0.0004
        ret = rng.normal(regime, vol)
        open_p = price
        close_p = price * (1.0 + ret)
        high_p = max(open_p, close_p) * (1.0 + abs(rng.normal(0.0, vol * 0.7)))
        low_p = min(open_p, close_p) * (1.0 - abs(rng.normal(0.0, vol * 0.7)))
        volume = rng.lognormal(mean=3.5, sigma=0.8)
        candles.append({
            "start_ms": start_ms + i * 60_000,
            "open": open_p,
            "high": high_p,
            "low": low_p,
            "close": close_p,
            "volume": volume,
            "turnover": volume * close_p,
        })
        price = close_p
    return candles


def test_synthetic_candles_generation():
    candles = generate_synthetic_candles(num_bars=150, seed=123)
    assert len(candles) == 150
    for c in candles:
        assert c["high"] >= c["low"]
        assert c["high"] >= max(c["open"], c["close"])
        assert c["low"] <= min(c["open"], c["close"])
        assert c["volume"] > 0


def test_feature_and_label_computation_causality():
    candles = generate_synthetic_candles(num_bars=200, seed=42)
    x, y, meta = compute_features_and_labels(candles, horizon=10)

    assert len(x) > 50
    assert x.shape[1] == len(FEATURE_NAMES)
    assert len(y) == len(x)
    assert np.all(np.isin(y, [0, 1]))
    assert np.all(np.isfinite(x))


def test_end_to_end_training_and_artifact_export(tmp_path):
    candles = generate_synthetic_candles(num_bars=300, seed=99)
    x, y, meta = compute_features_and_labels(candles, horizon=10)

    model_payload = train_champion_model(x, y, validation_fraction=0.25, embargo=10, random_seed=42)

    assert "model_version" in model_payload
    assert len(model_payload["features"]) == len(FEATURE_NAMES)
    assert "metrics" in model_payload
    assert 0.0 <= model_payload["metrics"]["brier"] <= 1.0
    assert 0.0 <= model_payload["metrics"]["ece"] <= 1.0

    out_file = tmp_path / "test_champion.json"
    export_model_artifact(model_payload, out_file, training_data_id="test_synthetic_300")

    assert out_file.exists()
    bundle = json.loads(out_file.read_text(encoding="utf-8"))
    assert "manifest" in bundle
    assert "model" in bundle

    manifest_data = bundle["manifest"]
    assert manifest_data["model_family"] == "platt_linear_meta_label"
    assert manifest_data["artifact_sha256"]


def test_tampered_model_artifact_detection(tmp_path):
    candles = generate_synthetic_candles(num_bars=200, seed=1)
    x, y, meta = compute_features_and_labels(candles, horizon=10)
    model_payload = train_champion_model(x, y, validation_fraction=0.25, embargo=10)

    out_file = tmp_path / "model.json"
    export_model_artifact(model_payload, out_file, training_data_id="test")

    # Load and tamper
    bundle = json.loads(out_file.read_text(encoding="utf-8"))
    bundle["model"]["intercept"] += 999.0
    tampered_file = tmp_path / "tampered.json"
    tampered_file.write_text(json.dumps(bundle), encoding="utf-8")

    from hashlib import sha256
    canonical = json.dumps(bundle["model"], sort_keys=True, separators=(",", ":"))
    new_sha = sha256(canonical.encode("utf-8")).hexdigest()
    assert new_sha != bundle["manifest"]["artifact_sha256"]
