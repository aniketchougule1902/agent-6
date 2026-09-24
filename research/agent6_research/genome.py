"""Deterministic, bounded challenger genomes for offline research only.

This module deliberately does not train, score, promote, or deploy models. It only
creates auditable candidate configurations from a validated parent. Selection must
still pass chronological OOS evaluation and the promotion registry gates.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import json
import random
from typing import Iterable

_ALLOWED_FAMILIES = {"lightgbm", "catboost"}
_ALLOWED_CALIBRATORS = {"platt", "isotonic"}


@dataclass(frozen=True)
class StrategyGenome:
    model_family: str
    calibrator: str
    no_trade_threshold: float
    learning_rate: float
    max_depth: int
    n_estimators: int
    min_samples_leaf: int
    feature_schema_version: str
    parent_id: str | None = None

    def validate(self) -> "StrategyGenome":
        if self.model_family not in _ALLOWED_FAMILIES:
            raise ValueError("unsupported model_family")
        if self.calibrator not in _ALLOWED_CALIBRATORS:
            raise ValueError("unsupported calibrator")
        if not 0.50 <= self.no_trade_threshold <= 0.90:
            raise ValueError("no_trade_threshold must be in [0.50, 0.90]")
        if not 0.005 <= self.learning_rate <= 0.20:
            raise ValueError("learning_rate must be in [0.005, 0.20]")
        if not 2 <= self.max_depth <= 12:
            raise ValueError("max_depth must be in [2, 12]")
        if not 50 <= self.n_estimators <= 2000:
            raise ValueError("n_estimators must be in [50, 2000]")
        if not 5 <= self.min_samples_leaf <= 500:
            raise ValueError("min_samples_leaf must be in [5, 500]")
        if not self.feature_schema_version.strip():
            raise ValueError("feature_schema_version is required")
        return self

    @property
    def genome_id(self) -> str:
        payload = json.dumps(asdict(self), sort_keys=True, separators=(",", ":"))
        return "genome-" + hashlib.sha256(payload.encode()).hexdigest()[:20]


def generate_challengers(parent: StrategyGenome, *, seed: int, count: int = 8) -> list[StrategyGenome]:
    """Generate deterministic bounded candidates without looking at labels/holdouts."""
    parent.validate()
    if not 1 <= count <= 64:
        raise ValueError("count must be in [1, 64]")
    rng = random.Random(seed)
    seen = {parent.genome_id}
    candidates: list[StrategyGenome] = []
    attempts = 0
    while len(candidates) < count and attempts < count * 20:
        attempts += 1
        candidate = StrategyGenome(
            model_family=parent.model_family if rng.random() < 0.8 else ("catboost" if parent.model_family == "lightgbm" else "lightgbm"),
            calibrator=parent.calibrator if rng.random() < 0.75 else ("isotonic" if parent.calibrator == "platt" else "platt"),
            no_trade_threshold=_clip(parent.no_trade_threshold + rng.choice((-0.04, -0.02, 0.02, 0.04)), 0.50, 0.90),
            learning_rate=_clip(parent.learning_rate * rng.choice((0.75, 0.9, 1.1, 1.25)), 0.005, 0.20),
            max_depth=int(_clip(parent.max_depth + rng.choice((-2, -1, 1, 2)), 2, 12)),
            n_estimators=int(_clip(parent.n_estimators + rng.choice((-200, -100, 100, 200)), 50, 2000)),
            min_samples_leaf=int(_clip(parent.min_samples_leaf + rng.choice((-20, -10, 10, 20)), 5, 500)),
            feature_schema_version=parent.feature_schema_version,
            parent_id=parent.genome_id,
        ).validate()
        if candidate.genome_id not in seen:
            seen.add(candidate.genome_id)
            candidates.append(candidate)
    if len(candidates) != count:
        raise RuntimeError("unable to generate requested unique challengers")
    return candidates


def assert_unique_genomes(genomes: Iterable[StrategyGenome]) -> None:
    ids = [g.validate().genome_id for g in genomes]
    if len(ids) != len(set(ids)):
        raise ValueError("duplicate challenger genome")


def _clip(value: float, low: float, high: float) -> float:
    return max(low, min(high, value))
