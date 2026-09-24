use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHaltReason {
    StaleFeed,
    StaleModel,
    CorruptData,
    AbnormalVolatility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSafetyTransition {
    Unchanged,
    Halted(RuntimeHaltReason),
    HaltReasonChanged { from: RuntimeHaltReason, to: RuntimeHaltReason },
    Recovered(RuntimeHaltReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeSafetyState {
    current_halt: Option<RuntimeHaltReason>,
}

impl RuntimeSafetyState {
    pub fn current_halt(&self) -> Option<RuntimeHaltReason> {
        self.current_halt
    }

    /// Applies a freshly evaluated safety result and returns only explicit state
    /// transitions. Repeated observations are idempotent and therefore cannot
    /// spam alarms; recovery always identifies the reason that was cleared.
    pub fn apply(&mut self, next: Option<RuntimeHaltReason>) -> RuntimeSafetyTransition {
        let transition = match (self.current_halt, next) {
            (None, None) | (Some(_), Some(_)) if self.current_halt == next => RuntimeSafetyTransition::Unchanged,
            (None, Some(reason)) => RuntimeSafetyTransition::Halted(reason),
            (Some(from), Some(to)) => RuntimeSafetyTransition::HaltReasonChanged { from, to },
            (Some(reason), None) => RuntimeSafetyTransition::Recovered(reason),
        };
        self.current_halt = next;
        transition
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuntimeSafetyLimits {
    pub max_feed_age_ms: u64,
    pub max_model_age_ms: u64,
    pub max_atr_bps: f64,
}

impl RuntimeSafetyLimits {
    pub fn validate(self) -> Result<Self> {
        ensure!(self.max_feed_age_ms > 0, "max feed age must be positive");
        ensure!(self.max_model_age_ms > 0, "max model age must be positive");
        ensure!(self.max_atr_bps.is_finite() && self.max_atr_bps > 0.0, "max ATR bps must be finite and positive");
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuntimeSafetyObservation {
    pub now_ms: u64,
    pub connected: bool,
    pub last_market_event_ms: u64,
    pub orderbook_event_ms: u64,
    pub model_required: bool,
    pub model_created_at_ms: Option<u64>,
    pub model_integrity_ok: bool,
    pub feature_integrity_ok: bool,
    pub last_price: Option<f64>,
    pub atr: Option<f64>,
}

/// Deterministic fail-closed runtime admission evaluator.
///
/// This function is intentionally side-effect free so the same observation can
/// be checked in live mode and deterministic replay. It does not mutate model
/// weights or enable execution. Callers should block *new* signal admission on
/// any returned halt while allowing existing lifecycle records to transition
/// explicitly and remain auditable.
pub fn evaluate_runtime_safety(
    observation: RuntimeSafetyObservation,
    limits: RuntimeSafetyLimits,
) -> Result<Option<RuntimeHaltReason>> {
    let limits = limits.validate()?;
    ensure!(observation.now_ms > 0, "runtime safety clock is required");

    if !observation.connected
        || observation.last_market_event_ms == 0
        || observation.orderbook_event_ms == 0
        || observation.last_market_event_ms > observation.now_ms
        || observation.orderbook_event_ms > observation.now_ms
        || observation.now_ms.saturating_sub(observation.last_market_event_ms) > limits.max_feed_age_ms
        || observation.now_ms.saturating_sub(observation.orderbook_event_ms) > limits.max_feed_age_ms
    {
        return Ok(Some(RuntimeHaltReason::StaleFeed));
    }

    if !observation.model_integrity_ok || !observation.feature_integrity_ok {
        return Ok(Some(RuntimeHaltReason::CorruptData));
    }

    match observation.model_created_at_ms {
        Some(created_at_ms) => {
            if created_at_ms == 0
                || created_at_ms > observation.now_ms
                || observation.now_ms.saturating_sub(created_at_ms) > limits.max_model_age_ms
            {
                return Ok(Some(RuntimeHaltReason::StaleModel));
            }
        }
        None if observation.model_required => return Ok(Some(RuntimeHaltReason::StaleModel)),
        None => {}
    }

    match (observation.last_price, observation.atr) {
        (Some(price), Some(atr)) => {
            if !price.is_finite() || price <= 0.0 || !atr.is_finite() || atr < 0.0 {
                return Ok(Some(RuntimeHaltReason::CorruptData));
            }
            let atr_bps = atr / price * 10_000.0;
            if !atr_bps.is_finite() {
                return Ok(Some(RuntimeHaltReason::CorruptData));
            }
            if atr_bps > limits.max_atr_bps {
                return Ok(Some(RuntimeHaltReason::AbnormalVolatility));
            }
        }
        (None, None) => return Ok(Some(RuntimeHaltReason::CorruptData)),
        _ => return Ok(Some(RuntimeHaltReason::CorruptData)),
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> RuntimeSafetyLimits {
        RuntimeSafetyLimits {
            max_feed_age_ms: 5_000,
            max_model_age_ms: 30 * 86_400_000,
            max_atr_bps: 150.0,
        }
    }

    fn healthy(now: u64) -> RuntimeSafetyObservation {
        RuntimeSafetyObservation {
            now_ms: now,
            connected: true,
            last_market_event_ms: now - 100,
            orderbook_event_ms: now - 100,
            model_required: true,
            model_created_at_ms: Some(now - 1_000),
            model_integrity_ok: true,
            feature_integrity_ok: true,
            last_price: Some(100.0),
            atr: Some(1.0),
        }
    }

    #[test]
    fn healthy_observation_allows_signal_admission() {
        assert_eq!(evaluate_runtime_safety(healthy(100_000), limits()).unwrap(), None);
    }

    #[test]
    fn stale_or_future_feed_fails_closed() {
        let mut o = healthy(100_000);
        o.last_market_event_ms = 94_999;
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::StaleFeed));
        let mut o = healthy(100_000);
        o.orderbook_event_ms = 100_001;
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::StaleFeed));
        let mut o = healthy(100_000);
        o.connected = false;
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::StaleFeed));
    }

    #[test]
    fn stale_future_or_required_missing_models_halt_instead_of_falling_back() {
        let now = 40 * 86_400_000;
        let mut o = healthy(now);
        o.model_created_at_ms = Some(now - 31 * 86_400_000);
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::StaleModel));
        let mut o = healthy(now);
        o.model_created_at_ms = Some(now + 1);
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::StaleModel));
        let mut o = healthy(now);
        o.model_created_at_ms = None;
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::StaleModel));
    }

    #[test]
    fn baseline_mode_can_explicitly_run_without_a_model() {
        let mut o = healthy(100_000);
        o.model_required = false;
        o.model_created_at_ms = None;
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), None);
    }

    #[test]
    fn corrupt_features_and_non_finite_market_values_fail_closed() {
        let mut o = healthy(100_000);
        o.feature_integrity_ok = false;
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::CorruptData));
        let mut o = healthy(100_000);
        o.last_price = Some(f64::NAN);
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::CorruptData));
        let mut o = healthy(100_000);
        o.atr = None;
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::CorruptData));
    }

    #[test]
    fn abnormal_volatility_halts_at_runtime_boundary() {
        let mut o = healthy(100_000);
        o.atr = Some(1.51);
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), Some(RuntimeHaltReason::AbnormalVolatility));
    }

    #[test]
    fn historical_clock_is_used_instead_of_wall_clock() {
        let historical_now = 1_000_000;
        let mut o = healthy(historical_now);
        o.model_created_at_ms = Some(historical_now - 10_000);
        assert_eq!(evaluate_runtime_safety(o, limits()).unwrap(), None);
    }

    #[test]
    fn halt_transitions_are_idempotent_and_recovery_is_explicit() {
        let mut state = RuntimeSafetyState::default();
        assert_eq!(state.apply(None), RuntimeSafetyTransition::Unchanged);
        assert_eq!(state.apply(Some(RuntimeHaltReason::StaleFeed)), RuntimeSafetyTransition::Halted(RuntimeHaltReason::StaleFeed));
        assert_eq!(state.apply(Some(RuntimeHaltReason::StaleFeed)), RuntimeSafetyTransition::Unchanged);
        assert_eq!(state.apply(Some(RuntimeHaltReason::StaleModel)), RuntimeSafetyTransition::HaltReasonChanged { from: RuntimeHaltReason::StaleFeed, to: RuntimeHaltReason::StaleModel });
        assert_eq!(state.current_halt(), Some(RuntimeHaltReason::StaleModel));
        assert_eq!(state.apply(None), RuntimeSafetyTransition::Recovered(RuntimeHaltReason::StaleModel));
        assert_eq!(state.current_halt(), None);
        assert_eq!(state.apply(None), RuntimeSafetyTransition::Unchanged);
    }
}