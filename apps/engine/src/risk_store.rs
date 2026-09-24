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

/// Write-ahead intent used to make the account/risk sidecar boundary
/// recoverable. Callers persist this *before* mutating the paper account, then
/// commit the risk snapshot after the account reaches `to_account_revision`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RiskCommitIntent {
    pub schema_version: u32,
    pub from_account_revision: u64,
    pub to_account_revision: u64,
    pub snapshot: RiskStateSnapshot,
}

impl RiskCommitIntent {
    pub const SCHEMA_VERSION: u32 = 1;
    fn validate(self) -> Result<Self> {
        ensure!(self.schema_version == Self::SCHEMA_VERSION, "unsupported risk commit-intent schema version");
        ensure!(self.to_account_revision == self.from_account_revision.checked_add(1).ok_or_else(|| anyhow::anyhow!("account revision overflow"))?, "risk commit intent must advance exactly one account revision");
        self.snapshot.validate()?;
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    Clean,
    AbortedUnappliedIntent,
    CompletedRiskCommit,
}

/// Crash-safe sidecar for the paper circuit breaker. The account revision is
/// stored with every snapshot so callers can fail closed if account and risk
/// state do not describe the same committed paper state. A write-ahead intent
/// makes a crash between the paper-account rename and risk-state rename
/// deterministic and recoverable rather than ambiguous.
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

    fn load_state_for_revision(&self, expected_revision: u64) -> Result<Option<RiskStateSnapshot>> {
        if !self.path.exists() { return Ok(None); }
        let state: PersistedRiskState = serde_json::from_slice(&fs::read(&self.path).context("read risk state")?)
            .context("decode risk state")?;
        let state = state.validate()?;
        ensure!(state.account_revision == expected_revision,
            "paper/risk revision mismatch: account={}, risk={}; entry admission must fail closed",
            expected_revision, state.account_revision);
        Ok(Some(state.snapshot))
    }

    pub fn persist(&self, account_revision: u64, snapshot: RiskStateSnapshot) -> Result<()> {
        let state = PersistedRiskState { schema_version: PersistedRiskState::SCHEMA_VERSION, account_revision, snapshot }.validate()?;
        atomic_json(&self.path, &state)
    }

    /// Durably records the intended one-revision transition before the paper
    /// account is changed. Existing unfinished intents fail closed.
    pub fn begin_commit(&self, from_account_revision: u64, snapshot: RiskStateSnapshot) -> Result<RiskCommitIntent> {
        ensure!(!self.intent_path.exists(), "unfinished paper/risk commit intent already exists");
        let intent = RiskCommitIntent {
            schema_version: RiskCommitIntent::SCHEMA_VERSION,
            from_account_revision,
            to_account_revision: from_account_revision.checked_add(1).ok_or_else(|| anyhow::anyhow!("account revision overflow"))?,
            snapshot,
        }.validate()?;
        atomic_json(&self.intent_path, &intent)?;
        Ok(intent)
    }

    /// Completes a prepared transition only after the caller has durably
    /// committed the paper account at the exact target revision.
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

    /// Resolves an intent after restart. If the account is still at the source
    /// revision, the mutation never committed and the intent is safely aborted.
    /// If the account reached the target revision, the risk sidecar is completed.
    /// Any other revision is ambiguous/corrupt and fails closed without deleting
    /// evidence needed for operator investigation.
    pub fn recover(&self, account_revision: u64) -> Result<RecoveryAction> {
        let Some(intent) = self.read_intent()? else {
            self.load_state_for_revision(account_revision)?;
            return Ok(RecoveryAction::Clean);
        };
        if account_revision == intent.from_account_revision {
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
    fn prepared_intent_blocks_normal_load_until_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.persist(4, snapshot(10)).unwrap();
        store.begin_commit(4, snapshot(11)).unwrap();
        assert!(store.load_for_revision(4).is_err());
        assert!(store.begin_commit(4, snapshot(12)).is_err());
    }

    #[test]
    fn recovery_aborts_when_account_mutation_never_committed() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.persist(4, snapshot(10)).unwrap();
        store.begin_commit(4, snapshot(11)).unwrap();
        assert_eq!(store.recover(4).unwrap(), RecoveryAction::AbortedUnappliedIntent);
        assert_eq!(store.load_for_revision(4).unwrap().unwrap().observed_at_ms, 10);
    }

    #[test]
    fn recovery_completes_sidecar_when_account_commit_won_race() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.persist(4, snapshot(10)).unwrap();
        store.begin_commit(4, snapshot(11)).unwrap();
        assert_eq!(store.recover(5).unwrap(), RecoveryAction::CompletedRiskCommit);
        assert_eq!(store.load_for_revision(5).unwrap().unwrap().observed_at_ms, 11);
    }

    #[test]
    fn ambiguous_revision_fails_closed_and_preserves_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("risk.json");
        let store = RiskStateStore::new(&path);
        store.persist(4, snapshot(10)).unwrap();
        store.begin_commit(4, snapshot(11)).unwrap();
        assert!(store.recover(6).is_err());
        assert!(path.with_extension("intent.json").exists());
        assert!(store.load_for_revision(6).is_err());
    }

    #[test]
    fn finish_requires_exact_prepared_target_revision() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.persist(9, snapshot(20)).unwrap();
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
