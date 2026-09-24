"""Deterministic post-trade critic records for offline learning/audit.

Critics diagnose already-finalized outcomes. They do not mutate model weights,
promote challengers, or manufacture labels. Records are content-addressed so the
same validated outcome produces the same immutable identity.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import json
import math
from typing import Literal

Outcome = Literal["tp", "sl", "expired", "invalidated", "reversed"]


@dataclass(frozen=True)
class PostTradeCriticRecord:
    schema_version: int
    critic_id: str
    signal_id: str
    model_version: str
    symbol: str
    timeframe: str
    regime: str
    opened_at_ms: int
    closed_at_ms: int
    outcome: Outcome
    entry_probability: float
    calibrated_probability: float
    net_return_bps: float
    mfe_bps: float
    mae_bps: float
    cost_bps: float
    diagnosis: tuple[str, ...]
    source_hash: str

    def to_json(self) -> str:
        return json.dumps(asdict(self), sort_keys=True, separators=(",", ":"))


def _required_text(name: str, value: str) -> str:
    value = value.strip()
    if not value:
        raise ValueError(f"{name} is required")
    return value


def _finite(name: str, value: float) -> float:
    if not math.isfinite(value):
        raise ValueError(f"{name} must be finite")
    return float(value)


def build_post_trade_critic(
    *, signal_id: str, model_version: str, symbol: str, timeframe: str,
    regime: str, opened_at_ms: int, closed_at_ms: int, outcome: Outcome,
    entry_probability: float, calibrated_probability: float,
    net_return_bps: float, mfe_bps: float, mae_bps: float, cost_bps: float,
    source_hash: str,
) -> PostTradeCriticRecord:
    signal_id = _required_text("signal_id", signal_id)
    model_version = _required_text("model_version", model_version)
    symbol = _required_text("symbol", symbol)
    timeframe = _required_text("timeframe", timeframe)
    regime = _required_text("regime", regime)
    source_hash = _required_text("source_hash", source_hash)
    if outcome not in {"tp", "sl", "expired", "invalidated", "reversed"}:
        raise ValueError("unsupported finalized outcome")
    if opened_at_ms <= 0 or closed_at_ms < opened_at_ms:
        raise ValueError("critic timestamps must be chronological")
    entry_probability = _finite("entry_probability", entry_probability)
    calibrated_probability = _finite("calibrated_probability", calibrated_probability)
    if not 0.0 <= entry_probability <= 1.0 or not 0.0 <= calibrated_probability <= 1.0:
        raise ValueError("probabilities must be in [0, 1]")
    net_return_bps = _finite("net_return_bps", net_return_bps)
    mfe_bps = _finite("mfe_bps", mfe_bps)
    mae_bps = _finite("mae_bps", mae_bps)
    cost_bps = _finite("cost_bps", cost_bps)
    if mfe_bps < 0 or mae_bps > 0 or cost_bps < 0:
        raise ValueError("MFE must be >=0, MAE <=0, and costs >=0")

    diagnosis: list[str] = []
    if abs(calibrated_probability - entry_probability) >= 0.10:
        diagnosis.append("material_calibration_adjustment")
    if outcome == "sl" and calibrated_probability >= 0.70:
        diagnosis.append("high_confidence_adverse_outcome")
    if outcome == "tp" and calibrated_probability < 0.50:
        diagnosis.append("low_confidence_favorable_outcome")
    if cost_bps > max(abs(net_return_bps), 1.0):
        diagnosis.append("cost_dominated_outcome")
    if mfe_bps >= 2.0 * max(abs(mae_bps), 1e-9) and net_return_bps <= 0:
        diagnosis.append("favorable_excursion_not_captured")
    if not diagnosis:
        diagnosis.append("no_rule_based_anomaly")

    canonical = {
        "schema_version": 1, "signal_id": signal_id, "model_version": model_version,
        "symbol": symbol, "timeframe": timeframe, "regime": regime,
        "opened_at_ms": opened_at_ms, "closed_at_ms": closed_at_ms, "outcome": outcome,
        "entry_probability": entry_probability, "calibrated_probability": calibrated_probability,
        "net_return_bps": net_return_bps, "mfe_bps": mfe_bps, "mae_bps": mae_bps,
        "cost_bps": cost_bps, "diagnosis": diagnosis, "source_hash": source_hash,
    }
    critic_id = "critic_" + hashlib.sha256(
        json.dumps(canonical, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    return PostTradeCriticRecord(critic_id=critic_id, diagnosis=tuple(diagnosis), **canonical)
