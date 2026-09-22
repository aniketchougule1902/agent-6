from __future__ import annotations

from dataclasses import asdict, dataclass
from hashlib import sha256
import json
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class ModelArtifactManifest:
    """Content-addressed metadata for an offline model candidate.

    The manifest intentionally carries no credentials and does not imply promotion.
    Promotion remains a separate validation-gated operation.
    """

    schema_version: int
    model_family: str
    feature_schema_version: str
    training_data_id: str
    code_revision: str
    created_at_ms: int
    artifact_sha256: str
    metrics: dict[str, float]
    params: dict[str, Any]

    def canonical_json(self) -> str:
        return json.dumps(asdict(self), sort_keys=True, separators=(",", ":"), allow_nan=False)

    def manifest_sha256(self) -> str:
        return sha256(self.canonical_json().encode("utf-8")).hexdigest()


def sha256_file(path: str | Path) -> str:
    digest = sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_manifest(
    *,
    artifact_path: str | Path,
    model_family: str,
    feature_schema_version: str,
    training_data_id: str,
    code_revision: str,
    created_at_ms: int,
    metrics: dict[str, float],
    params: dict[str, Any],
) -> ModelArtifactManifest:
    if not model_family.strip():
        raise ValueError("model_family must be non-empty")
    if not feature_schema_version.strip():
        raise ValueError("feature_schema_version must be non-empty")
    if not training_data_id.strip():
        raise ValueError("training_data_id must be non-empty")
    if not code_revision.strip():
        raise ValueError("code_revision must be non-empty")
    if created_at_ms < 0:
        raise ValueError("created_at_ms must be non-negative")
    for name, value in metrics.items():
        if not isinstance(value, (int, float)) or not float("-inf") < float(value) < float("inf"):
            raise ValueError(f"metric {name!r} must be finite")

    return ModelArtifactManifest(
        schema_version=1,
        model_family=model_family,
        feature_schema_version=feature_schema_version,
        training_data_id=training_data_id,
        code_revision=code_revision,
        created_at_ms=created_at_ms,
        artifact_sha256=sha256_file(artifact_path),
        metrics={name: float(value) for name, value in metrics.items()},
        params=dict(params),
    )


def verify_artifact(manifest: ModelArtifactManifest, artifact_path: str | Path) -> bool:
    """Fail closed when candidate bytes differ from the recorded digest."""

    return sha256_file(artifact_path) == manifest.artifact_sha256
