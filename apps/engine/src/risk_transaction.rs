use crate::{
    risk::RiskStateSnapshot,
    risk_store::{RecoveryAction, RiskCommitKind, RiskStateStore},
};
use anyhow::{ensure, Context, Result};

/// Coordinates the write-ahead paper/risk protocol around one account mutation.
///
/// The risk intent is durable before `mutate` runs. A successful mutation must
/// report the exact committed account revision, after which the risk sidecar is
/// finalized. If the mutation returns an error before committing, the prepared
/// intent is explicitly aborted against the unchanged source revision. If the
/// process crashes (or finalization fails) after the account commit, the intent
/// remains on disk so startup recovery can deterministically complete it.
///
/// This helper deliberately does not own paper-account mutation logic. Callers
/// must compute the target `RiskStateSnapshot` from the exact post-mutation
/// account/equity they intend to persist; guessing or reconstructing it later
/// would reopen the crash-consistency gap this protocol is meant to close.
pub fn commit_paper_risk<T, F>(
    store: &RiskStateStore,
    from_account_revision: u64,
    target_snapshot: RiskStateSnapshot,
    kind: RiskCommitKind,
    mutate: F,
) -> Result<T>
where
    F: FnOnce() -> Result<(T, u64)>,
{
    match kind {
        RiskCommitKind::Mutation => {
            store.begin_commit(from_account_revision, target_snapshot)?;
        }
        RiskCommitKind::SessionReset => {
            store.begin_session_reset(from_account_revision, target_snapshot)?;
        }
    }

    let (value, committed_revision) = match mutate() {
        Ok(result) => result,
        Err(error) => {
            let recovery = store
                .recover(from_account_revision)
                .context("paper mutation failed and prepared risk intent could not be aborted")?;
            ensure!(
                recovery == RecoveryAction::AbortedUnappliedIntent,
                "paper mutation failed but risk recovery did not abort the unapplied intent"
            );
            return Err(error).context("paper mutation failed before durable risk commit");
        }
    };

    // Do not attempt to delete/abort the intent if this fails. The paper account
    // may already be durable; preserving the intent is what makes restart
    // recovery deterministic and fail-closed.
    store
        .finish_commit(committed_revision)
        .context("paper account committed but risk sidecar finalization failed; startup recovery required")?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::{RiskLimits, SessionRiskCircuitBreaker};

    fn snapshot(ts: u64, equity: f64) -> RiskStateSnapshot {
        let mut guard = SessionRiskCircuitBreaker::new(
            1000.0,
            RiskLimits {
                max_daily_loss_fraction: 0.05,
                max_drawdown_fraction: 0.03,
            },
        )
        .unwrap();
        guard.observe_equity(equity).unwrap();
        guard.snapshot(ts).unwrap()
    }

    #[test]
    fn successful_mutation_advances_account_bound_risk_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(7, snapshot(10, 1000.0)).unwrap();

        let value = commit_paper_risk(
            &store,
            7,
            snapshot(11, 990.0),
            RiskCommitKind::Mutation,
            || Ok(("committed", 8)),
        )
        .unwrap();

        assert_eq!(value, "committed");
        let restored = store.load_for_revision(8).unwrap().unwrap();
        assert_eq!(restored.last_equity, 990.0);
        assert_eq!(restored.observed_at_ms, 11);
    }

    #[test]
    fn mutation_error_aborts_only_the_unapplied_intent() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(3, snapshot(10, 1000.0)).unwrap();

        let result = commit_paper_risk::<(), _>(
            &store,
            3,
            snapshot(11, 995.0),
            RiskCommitKind::Mutation,
            || anyhow::bail!("simulated account write failure"),
        );
        assert!(result.is_err());
        let restored = store.load_for_revision(3).unwrap().unwrap();
        assert_eq!(restored.last_equity, 1000.0);
        assert_eq!(restored.observed_at_ms, 10);
    }

    #[test]
    fn wrong_committed_revision_leaves_intent_for_fail_closed_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(12, snapshot(10, 1000.0)).unwrap();

        let result = commit_paper_risk(
            &store,
            12,
            snapshot(11, 980.0),
            RiskCommitKind::Mutation,
            || Ok(((), 14)),
        );
        assert!(result.is_err());
        assert!(store.load_for_revision(12).is_err());
        assert_eq!(
            store.recover(13).unwrap(),
            RecoveryAction::CompletedRiskCommit
        );
        assert_eq!(
            store.load_for_revision(13).unwrap().unwrap().last_equity,
            980.0
        );
    }

    #[test]
    fn session_reset_uses_explicit_reset_transition() {
        let dir = tempfile::tempdir().unwrap();
        let store = RiskStateStore::new(dir.path().join("risk.json"));
        store.initialize(20, snapshot(10, 1000.0)).unwrap();

        commit_paper_risk(
            &store,
            20,
            snapshot(100, 1000.0),
            RiskCommitKind::SessionReset,
            || Ok(((), 21)),
        )
        .unwrap();
        assert_eq!(
            store.load_for_revision(21).unwrap().unwrap().observed_at_ms,
            100
        );
    }
}