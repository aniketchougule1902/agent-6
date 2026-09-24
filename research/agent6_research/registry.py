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
    Before any promotion/rollback, the current artifact and audit/pointer linkage
    are revalidated so a missing/tampered champion cannot be silently replaced.
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
        if pointer.promoted_at_ms < 0 or len(pointer.manifest_sha256) != 64 or len(pointer.artifact_sha256) != 64:
            raise ValueError("invalid champion pointer")
        return pointer

    def current_verified(self) -> ChampionPointer | None:
        """Return the champion only when pointer, artifact and audit agree."""
        pointer = self.current()
        if pointer is None:
            if self.audit_path.exists() and self.audit_path.read_text().strip():
                raise ValueError("promotion audit exists without champion pointer")
            return None
        artifact = Path(pointer.artifact_path)
        if not artifact.is_file():
            raise ValueError("champion artifact is missing")
        if sha256(artifact.read_bytes()).hexdigest() != pointer.artifact_sha256:
            raise ValueError("champion artifact hash mismatch")
        rows = self._audit_rows()
        if not rows:
            raise ValueError("champion pointer exists without promotion audit")
        last = rows[-1]
        if last.get("resulting_champion_manifest_sha256") != pointer.manifest_sha256:
            raise ValueError("champion pointer does not match promotion audit")
        if last.get("at_ms") != pointer.promoted_at_ms:
            raise ValueError("champion pointer timestamp does not match promotion audit")
        return pointer

    def bootstrap(self, manifest: ModelArtifactManifest, artifact_path: str | Path, at_ms: int) -> PromotionAuditRecord:
        if self.current_verified() is not None:
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
        current = self.current_verified()
        if current is None:
            raise ValueError("bootstrap a champion before evaluating challengers")
        if at_ms <= current.promoted_at_ms:
            raise ValueError("promotion decision timestamp must advance monotonically")
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
        current = self.current_verified()
        if current is None:
            raise ValueError("no champion to roll back")
        if at_ms <= current.promoted_at_ms:
            raise ValueError("rollback timestamp must advance monotonically")
        if not reason.strip():
            raise ValueError("rollback reason must be non-empty")
        self._validate_candidate(manifest, artifact_path, at_ms)
        target = manifest.manifest_sha256()
        prior_champions = {
            row.get("resulting_champion_manifest_sha256")
            for row in self._audit_rows()
            if row.get("accepted") is True
        }
        if target not in prior_champions:
            raise ValueError("rollback target was never an audited champion")
        pointer = self._pointer(manifest, artifact_path, at_ms)
        record = PromotionAuditRecord(1, "rollback", at_ms, target,
                                      current.manifest_sha256, pointer.manifest_sha256, True,
                                      (reason.strip(),), None, None)
        self._commit(pointer, record)
        return record

    def _audit_rows(self) -> list[dict]:
        if not self.audit_path.exists():
            return []
        rows: list[dict] = []
        previous_at = -1
        expected_champion: str | None = None
        for line_no, line in enumerate(self.audit_path.read_text().splitlines(), 1):
            if not line.strip():
                raise ValueError(f"blank promotion audit row at line {line_no}")
            try:
                row = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(f"corrupt promotion audit row at line {line_no}") from exc
            if row.get("schema_version") != 1 or row.get("action") not in {"bootstrap", "promote", "reject", "rollback"}:
                raise ValueError(f"invalid promotion audit row at line {line_no}")
            at_ms = row.get("at_ms")
            if not isinstance(at_ms, int) or at_ms <= previous_at:
                raise ValueError("promotion audit timestamps must be strictly increasing")
            prior = row.get("prior_champion_manifest_sha256")
            if rows and prior != expected_champion:
                raise ValueError("promotion audit champion chain is broken")
            if not rows and row.get("action") != "bootstrap":
                raise ValueError("promotion audit must begin with bootstrap")
            resulting = row.get("resulting_champion_manifest_sha256")
            if not isinstance(resulting, str) or len(resulting) != 64:
                raise ValueError("promotion audit has invalid resulting champion")
            expected_champion = resulting
            previous_at = at_ms
            rows.append(row)
        return rows

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
