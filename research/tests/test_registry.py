import json

import pytest

from agent6_research.artifacts import build_manifest
from agent6_research.promotion import EvaluationReport, PromotionPolicy
from agent6_research.registry import ChampionRegistry


def report(expectancy: float, drawdown: float = 8.0) -> EvaluationReport:
    return EvaluationReport(600, expectancy, 1.3, drawdown, 0.16, 0.04, expectancy - 0.02, expectancy - 0.01)


def manifest(tmp_path, name: str, created: int):
    path = tmp_path / f"{name}.bin"
    path.write_bytes(name.encode())
    return path, build_manifest(artifact_path=path, model_family="lightgbm",
        feature_schema_version="v1", training_data_id=f"walk-forward-{name}", code_revision="abc123",
        created_at_ms=created, metrics={"brier": 0.16}, params={"seed": 7})


def test_registry_promotes_only_after_policy_and_preserves_rejection(tmp_path):
    registry = ChampionRegistry(tmp_path / "registry")
    champion_path, champion = manifest(tmp_path, "champion", 100)
    registry.bootstrap(champion, champion_path, 200)
    weak_path, weak = manifest(tmp_path, "weak", 210)
    rejected = registry.evaluate_and_promote(candidate_manifest=weak, candidate_path=weak_path,
        candidate_report=report(0.01), champion_report=report(0.08), policy=PromotionPolicy(), at_ms=300)
    assert not rejected.accepted and rejected.action == "reject"
    assert registry.current_verified().manifest_sha256 == champion.manifest_sha256()
    strong_path, strong = manifest(tmp_path, "strong", 310)
    promoted = registry.evaluate_and_promote(candidate_manifest=strong, candidate_path=strong_path,
        candidate_report=report(0.12), champion_report=report(0.08), policy=PromotionPolicy(), at_ms=400)
    assert promoted.accepted and promoted.action == "promote"
    assert registry.current_verified().manifest_sha256 == strong.manifest_sha256()
    rows = [json.loads(line) for line in registry.audit_path.read_text().splitlines()]
    assert [row["action"] for row in rows] == ["bootstrap", "reject", "promote"]


def test_registry_fails_closed_on_tamper_future_artifact_and_missing_bootstrap(tmp_path):
    registry = ChampionRegistry(tmp_path / "registry")
    path, candidate = manifest(tmp_path, "candidate", 100)
    with pytest.raises(ValueError, match="bootstrap"):
        registry.evaluate_and_promote(candidate_manifest=candidate, candidate_path=path,
            candidate_report=report(0.2), champion_report=report(0.1), policy=PromotionPolicy(), at_ms=200)
    path.write_bytes(b"tampered")
    with pytest.raises(ValueError, match="hash mismatch"):
        registry.bootstrap(candidate, path, 200)
    future_path, future = manifest(tmp_path, "future", 500)
    with pytest.raises(ValueError, match="future"):
        registry.bootstrap(future, future_path, 400)


def test_rollback_is_explicit_hashed_and_audited(tmp_path):
    registry = ChampionRegistry(tmp_path / "registry")
    old_path, old = manifest(tmp_path, "old", 100)
    new_path, new = manifest(tmp_path, "new", 200)
    registry.bootstrap(old, old_path, 150)
    registry.evaluate_and_promote(candidate_manifest=new, candidate_path=new_path,
        candidate_report=report(0.14), champion_report=report(0.08), policy=PromotionPolicy(), at_ms=250)
    rollback = registry.rollback(manifest=old, artifact_path=old_path, at_ms=300, reason="shadow regression")
    assert rollback.action == "rollback"
    assert rollback.reasons == ("shadow regression",)
    assert registry.current_verified().manifest_sha256 == old.manifest_sha256()
    assert len(rollback.record_sha256()) == 64
    with pytest.raises(ValueError, match="reason"):
        registry.rollback(manifest=new, artifact_path=new_path, at_ms=400, reason="  ")


def test_current_verified_rejects_artifact_pointer_and_audit_tampering(tmp_path):
    registry = ChampionRegistry(tmp_path / "registry")
    path, champion = manifest(tmp_path, "champion", 100)
    registry.bootstrap(champion, path, 200)
    path.write_bytes(b"changed-after-promotion")
    with pytest.raises(ValueError, match="artifact hash mismatch"):
        registry.current_verified()

    path.write_bytes(b"champion")
    pointer = json.loads(registry.pointer_path.read_text())
    pointer["manifest_sha256"] = "0" * 64
    registry.pointer_path.write_text(json.dumps(pointer))
    with pytest.raises(ValueError, match="does not match promotion audit"):
        registry.current_verified()


def test_audit_chain_and_decision_time_are_fail_closed(tmp_path):
    registry = ChampionRegistry(tmp_path / "registry")
    old_path, old = manifest(tmp_path, "old", 100)
    registry.bootstrap(old, old_path, 200)
    weak_path, weak = manifest(tmp_path, "weak", 210)
    registry.evaluate_and_promote(candidate_manifest=weak, candidate_path=weak_path,
        candidate_report=report(0.01), champion_report=report(0.08), policy=PromotionPolicy(), at_ms=300)
    with pytest.raises(ValueError, match="timestamp"):
        registry.evaluate_and_promote(candidate_manifest=weak, candidate_path=weak_path,
            candidate_report=report(0.01), champion_report=report(0.08), policy=PromotionPolicy(), at_ms=300)

    rows = [json.loads(line) for line in registry.audit_path.read_text().splitlines()]
    rows[-1]["prior_champion_manifest_sha256"] = "f" * 64
    registry.audit_path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
    with pytest.raises(ValueError, match="chain is broken"):
        registry.current_verified()


def test_rollback_rejects_never_champion_candidate(tmp_path):
    registry = ChampionRegistry(tmp_path / "registry")
    old_path, old = manifest(tmp_path, "old", 100)
    never_path, never = manifest(tmp_path, "never", 150)
    registry.bootstrap(old, old_path, 200)
    with pytest.raises(ValueError, match="never an audited champion"):
        registry.rollback(manifest=never, artifact_path=never_path, at_ms=300, reason="operator test")
