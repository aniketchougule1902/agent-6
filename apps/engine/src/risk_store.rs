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

/// Crash-safe sidecar for the paper circuit breaker. The account revision is
/// stored with every snapshot so callers can fail closed if account and risk
/// state do not describe the same committed paper state.
pub struct RiskStateStore { path: PathBuf }

impl RiskStateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self { Self { path: path.into() } }

    pub fn load_for_revision(&self, expected_revision: u64) -> Result<Option<RiskStateSnapshot>> {
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
        if let Some(parent) = self.path.parent() { fs::create_dir_all(parent)?; }
        let tmp = self.path.with_extension("tmp");
        {
            let mut file = fs::File::create(&tmp).context("create temporary risk state")?;
            file.write_all(&serde_json::to_vec_pretty(&state)?).context("write risk state")?;
            file.sync_all().context("fsync risk state")?;
        }
        fs::rename(&tmp, &self.path).context("atomically replace risk state")?;
        sync_parent(&self.path)?;
        Ok(())
    }
}

fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs::File::open(parent) { dir.sync_all().context("fsync risk-state directory")?; }
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
