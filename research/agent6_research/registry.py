from __future__ import annotations

from dataclasses import asdict, dataclass
from hashlib import sha256
import json
import os
from pathlib import Path
from typing import Literal

from .artifacts import ModelArtifactManifest, verify_artifact
from .promotion import EvaluationReport, PromotionPolicy

Action = Literal["bootstrap", "promote", "reject", "rollback"]


@dataclass(frozen=True)
class ChampionPointer:
    schema_version: int
    manifest_sha256: str
    artifact_sha256: str
    artifact_path: str
    promoted_at_ms: int


@dataclass(frozen=True)
class PromotionAuditRecord:
    schema_version: int
    action: Action
    at_ms: int
    candidate_manifest_sha256: str
    prior_champion_manifest_sha256: str | None
    resulting_champion_manifest_sha256: str | None
    accepted: bool
    reasons: tuple[str, ...]
    candidate_report: EvaluationReport | None
    champion_report: EvaluationReport | None

    def canonical_json(self) -> str:
        return json.dumps(asdict(self), sort_keys=True, separators=(",", ":"), allow_nan=False)

    def record_sha256(self) -> str:
        return sha256(self.canonical_json().encode()).hexdigest()


class ChampionRegistry:
    """Offline, fail-closed champion pointer with append-only promotion history.

    This registry never trains models and never changes live weights. Callers must
    supply immutable artifact manifests plus reproducible evaluation reports.
    """

    def __init__(self, root: str | Path) -> None:
        self.root = Path(root)
        self.pointer_path = self.root / "champion.json"
        self.audit_path = self.root / "promotion-audit.jsonl"

    def current(self) -> ChampionPointer | None:
        if not self.pointer_path.exists():
            return None
        raw = json.loads(self.pointer_path.read_text())
        pointer = ChampionPointer(**raw)
        if pointer.schema_version != 1:
            raise ValueError("unsupported champion pointer schema")
        return pointer

    def bootstrap(self, manifest: ModelArtifactManifest, artifact_path: str | Path, at_ms: int) -> PromotionAuditRecord:
        if self.current() is not None:
            raise ValueError("champion already exists")
        self._validate_candidate(manifest, artifact_path, at_ms)
        pointer = self._pointer(manifest, artifact_path, at_ms)
        record = PromotionAuditRecord(1, "bootstrap", at_ms, manifest.manifest_sha256(), None,
                                      pointer.manifest_sha256, True, (), None, None)
        self._commit(pointer, record)
        return record

    def evaluate_and_promote(self, *, candidate_manifest: ModelArtifactManifest, candidate_path: str | Path,
                             candidate_report: EvaluationReport, champion_report: EvaluationReport,
                             policy: PromotionPolicy, at_ms: int) -> PromotionAuditRecord:
        current = self.current()
        if current is None:
            raise ValueError("bootstrap a champion before evaluating challengers")
        self._validate_candidate(candidate_manifest, candidate_path, at_ms)
        accepted, reasons = policy.accepts(candidate_report, champion_report)
        resulting = current.manifest_sha256
        action: Action = "reject"
        if accepted:
            pointer = self._pointer(candidate_manifest, candidate_path, at_ms)
            resulting = pointer.manifest_sha256
            action = "promote"
        else:
            pointer = current
        record = PromotionAuditRecord(1, action, at_ms, candidate_manifest.manifest_sha256(),
                                      current.manifest_sha256, resulting, accepted, tuple(reasons),
                                      candidate_report, champion_report)
        self._commit(pointer if accepted else None, record)
        return record

    def rollback(self, *, manifest: ModelArtifactManifest, artifact_path: str | Path, at_ms: int,
                 reason: str) -> PromotionAuditRecord:
        current = self.current()
        if current is None:
            raise ValueError("no champion to roll back")
        if not reason.strip():
            raise ValueError("rollback reason must be non-empty")
        self._validate_candidate(manifest, artifact_path, at_ms)
        pointer = self._pointer(manifest, artifact_path, at_ms)
        record = PromotionAuditRecord(1, "rollback", at_ms, manifest.manifest_sha256(),
                                      current.manifest_sha256, pointer.manifest_sha256, True,
                                      (reason.strip(),), None, None)
        self._commit(pointer, record)
        return record

    def _validate_candidate(self, manifest: ModelArtifactManifest, artifact_path: str | Path, at_ms: int) -> None:
        if at_ms < 0:
            raise ValueError("at_ms must be non-negative")
        if not verify_artifact(manifest, artifact_path):
            raise ValueError("artifact hash mismatch")
        if manifest.created_at_ms > at_ms:
            raise ValueError("artifact creation timestamp is in the future")

    @staticmethod
    def _pointer(manifest: ModelArtifactManifest, artifact_path: str | Path, at_ms: int) -> ChampionPointer:
        return ChampionPointer(1, manifest.manifest_sha256(), manifest.artifact_sha256,
                               str(Path(artifact_path).resolve()), at_ms)

    def _commit(self, pointer: ChampionPointer | None, record: PromotionAuditRecord) -> None:
        self.root.mkdir(parents=True, exist_ok=True)
        # Audit is append-only. fsync before pointer replacement so a champion can
        # never appear without a durable decision record.
        with self.audit_path.open("a", encoding="utf-8") as handle:
            handle.write(record.canonical_json() + "\n")
            handle.flush()
            os.fsync(handle.fileno())
        if pointer is None:
            return
        temp = self.pointer_path.with_suffix(".tmp")
        with temp.open("w", encoding="utf-8") as handle:
            json.dump(asdict(pointer), handle, sort_keys=True, separators=(",", ":"), allow_nan=False)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp, self.pointer_path)
