import json

import pytest

from agent6_research.artifacts import build_manifest, verify_artifact


def test_manifest_is_deterministic_and_verifies_candidate(tmp_path):
    artifact = tmp_path / "candidate.bin"
    artifact.write_bytes(b"agent-6-candidate-v1")

    kwargs = dict(
        artifact_path=artifact,
        model_family="lightgbm",
        feature_schema_version="features-v1",
        training_data_id="replay-sha256:abc123",
        code_revision="deadbeef",
        created_at_ms=1_700_000_000_000,
        metrics={"brier": 0.12, "auc": 0.71},
        params={"seed": 6, "num_leaves": 31},
    )
    first = build_manifest(**kwargs)
    second = build_manifest(**kwargs)

    assert first.canonical_json() == second.canonical_json()
    assert first.manifest_sha256() == second.manifest_sha256()
    assert len(first.artifact_sha256) == 64
    assert verify_artifact(first, artifact)
    json.loads(first.canonical_json())


def test_tampered_artifact_fails_closed(tmp_path):
    artifact = tmp_path / "candidate.bin"
    artifact.write_bytes(b"original")
    manifest = build_manifest(
        artifact_path=artifact,
        model_family="lightgbm",
        feature_schema_version="features-v1",
        training_data_id="dataset-1",
        code_revision="abc",
        created_at_ms=1,
        metrics={"brier": 0.2},
        params={},
    )

    artifact.write_bytes(b"tampered")
    assert not verify_artifact(manifest, artifact)


def test_manifest_rejects_nonfinite_metrics(tmp_path):
    artifact = tmp_path / "candidate.bin"
    artifact.write_bytes(b"model")
    with pytest.raises(ValueError, match="finite"):
        build_manifest(
            artifact_path=artifact,
            model_family="lightgbm",
            feature_schema_version="features-v1",
            training_data_id="dataset-1",
            code_revision="abc",
            created_at_ms=1,
            metrics={"auc": float("nan")},
            params={},
        )
