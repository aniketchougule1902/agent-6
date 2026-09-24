use crate::risk::RiskStateSnapshot;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::{Path, PathBuf}};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PersistedRiskState {
    pub schema_version: u32,
    pub account_revision: u64,
    pub snapshot: RiskStateSnapshot,
}

impl PersistedRiskState {
    pub const SCHEMA_VERSION: u32 = 1;
    pub fn validate(self) -> Result<Self> {
        ensure!(self.schema_version == Self::SCHEMA_VERSION, "unsupported persisted risk-state schema version");
        self.snapshot.validate()?;
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskCommitKind { Mutation, SessionReset }

/// Write-ahead intent used to make the account/risk sidecar boundary
/// recoverable. Callers persist this *before* mutating the paper account, then
/// commit the risk snapshot after the account reaches `to_account_revision`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RiskCommitIntent {
    pub schema_version: u32,
    pub from_account_revision: u64,
    pub to_account_revision: u64,
    pub kind: RiskCommitKind,
    pub snapshot: RiskStateSnapshot,
}

impl RiskCommitIntent {
    pub const SCHEMA_VERSION: u32 = 2;
    fn validate(self) -> Result<Self> {
        ensure!(self.schema_version == Self::SCHEMA_VERSION, "unsupported risk commit-intent schema version");
        ensure!(self.to_account_revision == self.from_account_revision.checked_add(1).ok_or_else(|| anyhow::anyhow!("account revision overflow"))?, "risk commit intent must advance exactly one account revision");
        self.snapshot.validate()?;
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction { Clean, AbortedUnappliedIntent, CompletedRiskCommit }

/// Crash-safe sidecar for the paper circuit breaker. Every prepared mutation is
/// now bound to the currently durable source revision and, for ordinary account
/// mutations, to a monotonic risk transition. This prevents a stale caller from
/// preparing against the wrong account revision or silently clearing a latched
/// breaker / lowering the recorded equity peak.
pub struct RiskStateStore { path: PathBuf, intent_path: PathBuf }

impl RiskStateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let intent_path = path.with_extension("intent.json");
        Self { path, intent_path }
    }

    pub fn load_for_revision(&self, expected_revision: u64) -> Result<Option<RiskStateSnapshot>> {
        ensure!(!self.intent_path.exists(), "unfinished paper/risk commit intent exists; recover before entry admission");
        self.load_state_for_revision(expected_revision)
    }

    fn load_persisted(&self) -> Result<Option<PersistedRiskState>> {
        if !self.path.exists() { return Ok(None); }
        let state: PersistedRiskState = serde_json::from_slice(&fs::read(&self.path).context("read risk state")?)
            .context("decode risk state")?;
        Ok(Some(state.validate()?))
    }

    fn load_state_for_revision(&self, expected_revision: u64) -> Result<Option<RiskStateSnapshot>> {
        let Some(state) = self.load_persisted()? else { return Ok(None); };
        ensure!(state.account_revision == expected_revision,
            "paper/risk revision mismatch: account={}, risk={}; entry admission must fail closed",
            expected_revision, state.account_revision);
        Ok(Some(state.snapshot))
    }

    pub fn persist(&self, account_revision: u64, snapshot: RiskStateSnapshot) -> Result<()> {
        let state = PersistedRiskState { schema_version: PersistedRiskState::SCHEMA_VERSION, account_revision, snapshot }.validate()?;
        atomic_json(&self.path, &state)
    }

    /// Initialize a risk sidecar only when no prior sidecar or unfinished intent
    /// exists. This is intentionally separate from mutation preparation so a
    /// missing/corrupt sidecar cannot be silently recreated during admission.
    pub fn initialize(&self, account_revision: u64, snapshot: RiskStateSnapshot) -> Result<()> {
        ensure!(!self.intent_path.exists(), "cannot initialize while a risk commit intent exists");
        ensure!(!self.path.exists(), "risk state already exists");
        self.persist(account_revision, snapshot)
    }

    /// Prepare an ordinary paper mutation. The source revision must exactly
    /// match durable risk state and safety memory must move monotonically.
    pub fn begin_commit(&self, from_account_revision: u64, snapshot: RiskStateSnapshot) -> Result<RiskCommitIntent> {
        self.begin(from_account_revision, snapshot, RiskCommitKind::Mutation)
    }

    /// Prepare an explicit paper-session reset. A reset may clear the old
    /// session latch/peak, but it is still revision-bound and crash recoverable.
    pub fn begin_session_reset(&self, from_account_revision: u64, snapshot: RiskStateSnapshot) -> Result<RiskCommitIntent> {
        self.begin(from_account_revision, snapshot, RiskCommitKind::SessionReset)
    }

    fn begin(&self, from_account_revision: u64, snapshot: RiskStateSnapshot, kind: RiskCommitKind) -> Result<RiskCommitIntent> {
        ensure!(!self.intent_path.exists(), "unfinished paper/risk commit intent already exists");
        let current = self.load_persisted()?.ok_or_else(|| anyhow::anyhow!("risk state missing; initialize explicitly before preparing paper mutations"))?;
        ensure!(current.account_revision == from_account_revision,
            "cannot prepare risk commit from stale revision {}: durable risk revision is {}",
            from_account_revision, current.account_revision);
        let snapshot = snapshot.validate()?;
        if kind == RiskCommitKind::Mutation { validate_monotonic_transition(current.snapshot, snapshot)?; }
        let intent = RiskCommitIntent {
            schema_version: RiskCommitIntent::SCHEMA_VERSION,
            from_account_revision,
            to_account_revision: from_account_revision.checked_add(1).ok_or_else(|| anyhow::anyhow!("account revision overflow"))?,
            kind,
            snapshot,
        }.validate()?;
        atomic_json(&self.intent_path, &intent)?;
        Ok(intent)
    }

    pub fn finish_commit(&self, committed_account_revision: u64) -> Result<()> {
        let intent = self.read_intent()?.ok_or_else(|| anyhow::anyhow!("no prepared risk commit intent"))?;
        ensure!(committed_account_revision == intent.to_account_revision,
            "cannot finish risk commit: account revision {} != prepared {}",
            committed_account_revision, intent.to_account_revision);
        self.persist(committed_account_revision, intent.snapshot)?;
        fs::remove_file(&self.intent_path).context("remove completed risk commit intent")?;
        sync_parent(&self.intent_path)?;
        Ok(())
    }

    pub fn recover(&self, account_revision: u64) -> Result<RecoveryAction> {
        let Some(intent) = self.read_intent()? else {
            self.load_state_for_revision(account_revision)?;
            return Ok(RecoveryAction::Clean);
        };
        if account_revision == intent.from_account_revision {
            // Also verify the source sidecar still matches the intent source;
            // otherwise deleting the intent would hide corruption.
            self.load_state_for_revision(account_revision)?.ok_or_else(|| anyhow::anyhow!("risk state missing during recovery"))?;
            fs::remove_file(&self.intent_path).context("abort unapplied risk commit intent")?;
            sync_parent(&self.intent_path)?;
            return Ok(RecoveryAction::AbortedUnappliedIntent);
        }
        if account_revision == intent.to_account_revision {
            self.persist(account_revision, intent.snapshot)?;
            fs::remove_file(&self.intent_path).context("remove recovered risk commit intent")?;
            sync_parent(&self.intent_path)?;
            return Ok(RecoveryAction::CompletedRiskCommit);
        }
        anyhow::bail!("ambiguous paper/risk recovery: account revision {} is neither intent source {} nor target {}; admission must remain halted", account_revision, intent.from_account_revision, intent.to_account_revision)
    }

    fn read_intent(&self) -> Result<Option<RiskCommitIntent>> {
        if !self.intent_path.exists() { return Ok(None); }
        let intent: RiskCommitIntent = serde_json::from_slice(&fs::read(&self.intent_path).context("read risk commit intent")?)
            .context("decode risk commit intent")?;
        Ok(Some(intent.validate()?))
    }
}

fn validate_monotonic_transition(current: RiskStateSnapshot, next: RiskStateSnapshot) -> Result<()> {
    ensure!(next.limits == current.limits, "ordinary risk mutation cannot change circuit-breaker limits");
    ensure!(next.session_start_equity == current.session_start_equity, "ordinary risk mutation cannot change session start equity");
    ensure!(next.peak_equity + f64::EPSILON >= current.peak_equity, "ordinary risk mutation cannot lower recorded peak equity");
    ensure!(next.observed_at_ms >= current.observed_at_ms, "ordinary risk mutation cannot move observation time backwards");
    if let Some(halt) = current.halt {
        ensure!(next.halt == Some(halt), "ordinary risk mutation cannot clear or replace a latched halt; reset the session explicitly");
    }
    Ok(())
}

fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp).context("create temporary durable state")?;
        file.write_all(&serde_json::to_vec_pretty(value)?).context("write durable state")?;
        file.sync_all().context("fsync durable state")?;
    }
    fs::rename(&tmp, path).context("atomically replace durable state")?;
    sync_parent(path)?;
    Ok(())
}

fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs::File::open(parent) { dir.sync_all().context("fsync durable-state directory")?; }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::{RiskHaltReason, RiskLimits, SessionRiskCircuitBreaker};

    fn snapshot(ts: u64) -> RiskStateSnapshot {
        let mut guard = SessionRiskCircuitBreaker::new(1000.0, RiskLimits { max_daily_loss_fraction: 0.05, max_drawdown_fraction: 0.03 }).unwrap();
        guard.observe_equity(1100.0).unwrap();
        assert_eq!(guard.observe_equity(1060.0).unwrap(), Some(RiskHaltReason::Drawdown));
        guard.snapshot(ts).unwrap()
    }
    fn safe_snapshot(ts: u64, equity: f64) -> RiskStateSnapshot {
        let mut guard = SessionRiskCircuitBreaker::new(1000.0, RiskLimits { max_daily_loss_fraction: 0.05, max_drawdown_fraction: 0.03 }).unwrap();
        guard.observe_equity(equity).unwrap();
        guard.snapshot(ts).unwrap()
    }

    #[test]
    fn round_trip_is_revision_bound_and_preserves_latch() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.persist(7, snapshot(1234)).unwrap();
        let restored = store.load_for_revision(7).unwrap().unwrap();
        assert_eq!(restored.halt, Some(RiskHaltReason::Drawdown));
        assert_eq!(restored.peak_equity, 1100.0);
        assert!(store.load_for_revision(8).is_err());
    }

    #[test]
    fn corrupt_unknown_and_unsafe_state_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("risk.json");
        let store = RiskStateStore::new(&path);
        fs::write(&path, b"not-json").unwrap();
        assert!(store.load_for_revision(1).is_err());
        let mut state = PersistedRiskState { schema_version: 99, account_revision: 1, snapshot: snapshot(1) };
        fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(store.load_for_revision(1).is_err());
        state.schema_version = 1;
        state.snapshot.halt = None;
        fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(store.load_for_revision(1).is_err());
    }

    #[test]
    fn initialize_is_explicit_and_single_use() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(4, safe_snapshot(10, 1000.0)).unwrap();
        assert!(store.initialize(4, safe_snapshot(11, 1000.0)).is_err());
        assert_eq!(store.load_for_revision(4).unwrap().unwrap().observed_at_ms, 10);
    }

    #[test]
    fn prepare_requires_exact_durable_source_revision() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        assert!(store.begin_commit(4, safe_snapshot(11, 1000.0)).is_err());
        store.initialize(4, safe_snapshot(10, 1000.0)).unwrap();
        assert!(store.begin_commit(3, safe_snapshot(11, 1000.0)).is_err());
        assert!(store.begin_commit(5, safe_snapshot(11, 1000.0)).is_err());
        store.begin_commit(4, safe_snapshot(11, 1000.0)).unwrap();
    }

    #[test]
    fn ordinary_mutation_cannot_erase_safety_memory() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(4, snapshot(10)).unwrap();
        assert!(store.begin_commit(4, safe_snapshot(11, 1000.0)).is_err());

        let mut lower_peak = snapshot(11);
        lower_peak.peak_equity = 1090.0;
        lower_peak.last_equity = 1060.0;
        assert!(store.begin_commit(4, lower_peak).is_err());

        let mut backwards = snapshot(9);
        backwards.observed_at_ms = 9;
        assert!(store.begin_commit(4, backwards).is_err());
    }

    #[test]
    fn explicit_session_reset_may_clear_prior_latch_but_remains_revision_bound() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(4, snapshot(10)).unwrap();
        let fresh = safe_snapshot(20, 1000.0);
        let intent = store.begin_session_reset(4, fresh).unwrap();
        assert_eq!(intent.kind, RiskCommitKind::SessionReset);
        store.finish_commit(5).unwrap();
        let restored = store.load_for_revision(5).unwrap().unwrap();
        assert_eq!(restored.halt, None);
        assert_eq!(restored.peak_equity, 1000.0);
    }

    #[test]
    fn prepared_intent_blocks_normal_load_until_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(4, snapshot(10)).unwrap();
        store.begin_commit(4, snapshot(11)).unwrap();
        assert!(store.load_for_revision(4).is_err());
        assert!(store.begin_commit(4, snapshot(12)).is_err());
    }

    #[test]
    fn recovery_aborts_when_account_mutation_never_committed() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(4, snapshot(10)).unwrap();
        store.begin_commit(4, snapshot(11)).unwrap();
        assert_eq!(store.recover(4).unwrap(), RecoveryAction::AbortedUnappliedIntent);
        assert_eq!(store.load_for_revision(4).unwrap().unwrap().observed_at_ms, 10);
    }

    #[test]
    fn recovery_completes_sidecar_when_account_commit_won_race() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(4, snapshot(10)).unwrap();
        store.begin_commit(4, snapshot(11)).unwrap();
        assert_eq!(store.recover(5).unwrap(), RecoveryAction::CompletedRiskCommit);
        assert_eq!(store.load_for_revision(5).unwrap().unwrap().observed_at_ms, 11);
    }

    #[test]
    fn ambiguous_revision_fails_closed_and_preserves_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("risk.json");
        let store = RiskStateStore::new(&path);
        store.initialize(4, snapshot(10)).unwrap();
        store.begin_commit(4, snapshot(11)).unwrap();
        assert!(store.recover(6).is_err());
        assert!(path.with_extension("intent.json").exists());
        assert!(store.load_for_revision(6).is_err());
    }

    #[test]
    fn finish_requires_exact_prepared_target_revision() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(9, snapshot(20)).unwrap();
        store.begin_commit(9, snapshot(21)).unwrap();
        assert!(store.finish_commit(9).is_err());
        store.finish_commit(10).unwrap();
        assert_eq!(store.load_for_revision(10).unwrap().unwrap().observed_at_ms, 21);
    }

    #[test]
    fn replacement_never_leaves_temp_file_after_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("risk.json");
        let store = RiskStateStore::new(&path);
        store.persist(1, snapshot(10)).unwrap();
        store.persist(2, snapshot(11)).unwrap();
        assert_eq!(store.load_for_revision(2).unwrap().unwrap().observed_at_ms, 11);
        assert!(!path.with_extension("tmp").exists());
    }
}
