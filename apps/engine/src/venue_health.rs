use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

/// Health is deliberately advisory/context-only. A secondary venue becoming
/// stale or isolated must never halt or mutate the primary Bybit state path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VenueHealthState {
    Healthy,
    Stale,
    Isolated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueHealthSnapshot {
    pub state: VenueHealthState,
    pub last_event_ms: Option<u64>,
    pub last_receive_ms: Option<u64>,
    pub consecutive_failures: u32,
    pub consecutive_recovery_samples: u32,
}

/// Bounded state machine for optional secondary-venue context.
///
/// Failures isolate only the secondary source. Recovery requires multiple
/// consecutive fresh observations so a reconnecting/flapping venue cannot be
/// re-admitted on one lucky packet. No method here can affect primary feed
/// admission or the deterministic signal path.
#[derive(Debug, Clone)]
pub struct SecondaryVenueHealth {
    max_age_ms: u64,
    failures_to_isolate: u32,
    recovery_samples_required: u32,
    snapshot: VenueHealthSnapshot,
}

impl SecondaryVenueHealth {
    pub fn new(max_age_ms: u64, failures_to_isolate: u32, recovery_samples_required: u32) -> Result<Self> {
        ensure!(max_age_ms > 0, "secondary venue max age must be positive");
        ensure!(failures_to_isolate > 0, "secondary venue isolation threshold must be positive");
        ensure!(recovery_samples_required >= 2, "secondary venue recovery requires at least two fresh samples");
        Ok(Self {
            max_age_ms,
            failures_to_isolate,
            recovery_samples_required,
            snapshot: VenueHealthSnapshot {
                state: VenueHealthState::Stale,
                last_event_ms: None,
                last_receive_ms: None,
                consecutive_failures: 0,
                consecutive_recovery_samples: 0,
            },
        })
    }

    pub fn snapshot(&self) -> VenueHealthSnapshot { self.snapshot }

    /// Record a successfully parsed/admitted secondary observation. Exchange
    /// event time and local receive time must both be monotonic and fresh.
    pub fn observe_fresh(&mut self, event_ms: u64, receive_ms: u64) -> Result<VenueHealthState> {
        ensure!(event_ms > 0 && receive_ms > 0, "secondary venue timestamps are required");
        ensure!(receive_ms >= event_ms, "secondary venue receive time predates exchange event");
        ensure!(receive_ms - event_ms <= self.max_age_ms, "secondary venue observation is stale on arrival");
        if let Some(last) = self.snapshot.last_event_ms {
            ensure!(event_ms > last, "secondary venue event time must increase strictly");
        }
        if let Some(last) = self.snapshot.last_receive_ms {
            ensure!(receive_ms >= last, "secondary venue receive time moved backwards");
        }
        self.snapshot.last_event_ms = Some(event_ms);
        self.snapshot.last_receive_ms = Some(receive_ms);
        self.snapshot.consecutive_failures = 0;
        self.snapshot.consecutive_recovery_samples = self.snapshot.consecutive_recovery_samples.saturating_add(1);
        if self.snapshot.state != VenueHealthState::Healthy
            && self.snapshot.consecutive_recovery_samples >= self.recovery_samples_required
        {
            self.snapshot.state = VenueHealthState::Healthy;
            self.snapshot.consecutive_recovery_samples = 0;
        }
        Ok(self.snapshot.state)
    }

    /// Mark a parser/sequence/socket/clock failure for the optional venue.
    /// Repeated failures isolate that venue only.
    pub fn record_failure(&mut self) -> VenueHealthState {
        self.snapshot.consecutive_recovery_samples = 0;
        self.snapshot.consecutive_failures = self.snapshot.consecutive_failures.saturating_add(1);
        self.snapshot.state = if self.snapshot.consecutive_failures >= self.failures_to_isolate {
            VenueHealthState::Isolated
        } else {
            VenueHealthState::Stale
        };
        self.snapshot.state
    }

    /// Time-based watchdog. Once the last receive is too old, context is not
    /// eligible even if the socket has not explicitly failed.
    pub fn tick(&mut self, now_ms: u64) -> Result<VenueHealthState> {
        ensure!(now_ms > 0, "secondary venue watchdog time is required");
        if let Some(last) = self.snapshot.last_receive_ms {
            ensure!(now_ms >= last, "secondary venue watchdog time moved backwards");
            if now_ms - last > self.max_age_ms {
                self.snapshot.consecutive_recovery_samples = 0;
                self.snapshot.state = VenueHealthState::Stale;
            }
        }
        Ok(self.snapshot.state)
    }

    pub fn context_eligible(&self) -> bool { self.snapshot.state == VenueHealthState::Healthy }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_and_single_sample_fail_closed_until_recovery_quorum() {
        let mut h = SecondaryVenueHealth::new(250, 2, 2).unwrap();
        assert_eq!(h.snapshot().state, VenueHealthState::Stale);
        assert!(!h.context_eligible());
        assert_eq!(h.observe_fresh(1_000, 1_010).unwrap(), VenueHealthState::Stale);
        assert!(!h.context_eligible());
        assert_eq!(h.observe_fresh(1_100, 1_110).unwrap(), VenueHealthState::Healthy);
        assert!(h.context_eligible());
    }

    #[test]
    fn repeated_failures_isolate_only_secondary_and_recovery_is_debounced() {
        let mut h = SecondaryVenueHealth::new(250, 2, 3).unwrap();
        h.observe_fresh(1_000, 1_010).unwrap();
        h.observe_fresh(1_100, 1_110).unwrap();
        h.observe_fresh(1_200, 1_210).unwrap();
        assert!(h.context_eligible());
        assert_eq!(h.record_failure(), VenueHealthState::Stale);
        assert_eq!(h.record_failure(), VenueHealthState::Isolated);
        assert!(!h.context_eligible());
        assert_eq!(h.observe_fresh(1_300, 1_310).unwrap(), VenueHealthState::Isolated);
        assert_eq!(h.observe_fresh(1_400, 1_410).unwrap(), VenueHealthState::Isolated);
        assert_eq!(h.observe_fresh(1_500, 1_510).unwrap(), VenueHealthState::Healthy);
        assert!(h.context_eligible());
    }

    #[test]
    fn watchdog_removes_stale_context_without_poisoning_sequence_history() {
        let mut h = SecondaryVenueHealth::new(100, 2, 2).unwrap();
        h.observe_fresh(1_000, 1_010).unwrap();
        h.observe_fresh(1_050, 1_060).unwrap();
        assert!(h.context_eligible());
        assert_eq!(h.tick(1_161).unwrap(), VenueHealthState::Stale);
        assert!(!h.context_eligible());
        assert_eq!(h.observe_fresh(1_170, 1_180).unwrap(), VenueHealthState::Stale);
        assert_eq!(h.observe_fresh(1_190, 1_200).unwrap(), VenueHealthState::Healthy);
    }

    #[test]
    fn malformed_or_reordered_timestamps_fail_closed() {
        let mut h = SecondaryVenueHealth::new(100, 2, 2).unwrap();
        assert!(h.observe_fresh(0, 10).is_err());
        assert!(h.observe_fresh(100, 99).is_err());
        assert!(h.observe_fresh(100, 250).is_err());
        h.observe_fresh(100, 110).unwrap();
        assert!(h.observe_fresh(100, 120).is_err());
        assert!(h.tick(109).is_err());
    }
}
