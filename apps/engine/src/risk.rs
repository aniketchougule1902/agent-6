use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RiskLimits {
    pub max_daily_loss_fraction: f64,
    pub max_drawdown_fraction: f64,
}
impl RiskLimits {
    pub fn validate(self) -> Result<Self> {
        if !self.max_daily_loss_fraction.is_finite() || !(0.0..1.0).contains(&self.max_daily_loss_fraction) { bail!("max_daily_loss_fraction must be finite and in (0, 1)"); }
        if !self.max_drawdown_fraction.is_finite() || !(0.0..1.0).contains(&self.max_drawdown_fraction) { bail!("max_drawdown_fraction must be finite and in (0, 1)"); }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskHaltReason { DailyLoss, Drawdown }
impl std::fmt::Display for RiskHaltReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(match self { Self::DailyLoss => "session loss limit reached", Self::Drawdown => "session drawdown limit reached" }) }
}

/// Durable representation of the latched risk state. The schema version is
/// explicit so future changes cannot silently reinterpret an older state file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RiskStateSnapshot {
    pub schema_version: u32,
    pub limits: RiskLimits,
    pub session_start_equity: f64,
    pub peak_equity: f64,
    pub last_equity: f64,
    pub halt: Option<RiskHaltReason>,
    pub observed_at_ms: u64,
}
impl RiskStateSnapshot {
    pub const SCHEMA_VERSION: u32 = 1;
    pub fn validate(self) -> Result<Self> {
        ensure!(self.schema_version == Self::SCHEMA_VERSION, "unsupported risk snapshot schema version");
        self.limits.validate()?;
        validate_equity(self.session_start_equity)?;
        validate_equity(self.peak_equity)?;
        validate_equity(self.last_equity)?;
        ensure!(self.peak_equity >= self.session_start_equity, "risk snapshot peak cannot be below session start");
        ensure!(self.peak_equity >= self.last_equity, "risk snapshot peak cannot be below last equity");
        ensure!(self.observed_at_ms > 0, "risk snapshot timestamp is required");
        let daily_loss = (self.session_start_equity - self.last_equity) / self.session_start_equity;
        let drawdown = (self.peak_equity - self.last_equity) / self.peak_equity;
        if self.halt.is_none() { ensure!(daily_loss < self.limits.max_daily_loss_fraction && drawdown < self.limits.max_drawdown_fraction, "unlatched risk snapshot already breaches configured limits"); }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct SessionRiskCircuitBreaker {
    limits: RiskLimits,
    session_start_equity: f64,
    peak_equity: f64,
    last_equity: f64,
    halt: Option<RiskHaltReason>,
}
impl SessionRiskCircuitBreaker {
    pub fn new(session_start_equity: f64, limits: RiskLimits) -> Result<Self> {
        let limits = limits.validate()?;
        validate_equity(session_start_equity)?;
        Ok(Self { limits, session_start_equity, peak_equity: session_start_equity, last_equity: session_start_equity, halt: None })
    }
    pub fn restore(snapshot: RiskStateSnapshot) -> Result<Self> {
        let s = snapshot.validate()?;
        Ok(Self { limits: s.limits, session_start_equity: s.session_start_equity, peak_equity: s.peak_equity, last_equity: s.last_equity, halt: s.halt })
    }
    pub fn snapshot(&self, observed_at_ms: u64) -> Result<RiskStateSnapshot> {
        RiskStateSnapshot { schema_version: RiskStateSnapshot::SCHEMA_VERSION, limits: self.limits, session_start_equity: self.session_start_equity, peak_equity: self.peak_equity, last_equity: self.last_equity, halt: self.halt, observed_at_ms }.validate()
    }
    pub fn observe_equity(&mut self, equity: f64) -> Result<Option<RiskHaltReason>> {
        validate_equity(equity)?;
        self.last_equity = equity;
        self.peak_equity = self.peak_equity.max(equity);
        if self.halt.is_some() { return Ok(self.halt); }
        let daily_loss = (self.session_start_equity - equity) / self.session_start_equity;
        let drawdown = (self.peak_equity - equity) / self.peak_equity;
        self.halt = if daily_loss >= self.limits.max_daily_loss_fraction { Some(RiskHaltReason::DailyLoss) } else if drawdown >= self.limits.max_drawdown_fraction { Some(RiskHaltReason::Drawdown) } else { None };
        Ok(self.halt)
    }
    pub fn is_halted(&self) -> bool { self.halt.is_some() }
}

pub fn evaluate_equity_history(session_start_equity: f64, historical_equities: impl IntoIterator<Item = f64>, current_equity: f64, limits: RiskLimits) -> Result<Option<RiskHaltReason>> {
    let mut guard = SessionRiskCircuitBreaker::new(session_start_equity, limits)?;
    for equity in historical_equities { guard.observe_equity(equity)?; }
    guard.observe_equity(current_equity)
}
fn validate_equity(equity: f64) -> Result<()> { if !equity.is_finite() || equity <= 0.0 { bail!("equity must be finite and positive"); } Ok(()) }

#[cfg(test)]
mod tests {
    use super::*;
    fn limits() -> RiskLimits { RiskLimits { max_daily_loss_fraction: 0.05, max_drawdown_fraction: 0.03 } }
    #[test] fn trips_daily_loss_past_threshold_and_latches() { let mut g=SessionRiskCircuitBreaker::new(1000.0,RiskLimits{max_daily_loss_fraction:0.05,max_drawdown_fraction:0.10}).unwrap(); assert_eq!(g.observe_equity(951.0).unwrap(),None); assert_eq!(g.observe_equity(949.0).unwrap(),Some(RiskHaltReason::DailyLoss)); assert!(g.is_halted()); assert_eq!(g.observe_equity(1100.0).unwrap(),Some(RiskHaltReason::DailyLoss)); }
    #[test] fn trips_drawdown_from_peak_before_daily_loss() { let mut g=SessionRiskCircuitBreaker::new(1000.0,limits()).unwrap(); assert_eq!(g.observe_equity(1100.0).unwrap(),None); assert_eq!(g.observe_equity(1066.0).unwrap(),Some(RiskHaltReason::Drawdown)); }
    #[test] fn rebuilt_history_preserves_peak_and_latched_breach() { assert_eq!(evaluate_equity_history(1000.0,[1100.0,1066.0,1120.0],1150.0,limits()).unwrap(),Some(RiskHaltReason::Drawdown)); }
    #[test] fn snapshot_round_trip_preserves_intratrade_peak_and_latch() { let mut g=SessionRiskCircuitBreaker::new(1000.0,limits()).unwrap(); assert_eq!(g.observe_equity(1120.0).unwrap(),None); assert_eq!(g.observe_equity(1080.0).unwrap(),Some(RiskHaltReason::Drawdown)); let encoded=serde_json::to_vec(&g.snapshot(1234).unwrap()).unwrap(); let decoded:RiskStateSnapshot=serde_json::from_slice(&encoded).unwrap(); let mut restored=SessionRiskCircuitBreaker::restore(decoded).unwrap(); assert!(restored.is_halted()); assert_eq!(restored.observe_equity(1200.0).unwrap(),Some(RiskHaltReason::Drawdown)); let s=restored.snapshot(1235).unwrap(); assert_eq!(s.peak_equity,1200.0); assert_eq!(s.last_equity,1200.0); }
    #[test] fn snapshot_rejects_corrupt_or_unsafe_state() { let base=RiskStateSnapshot{schema_version:1,limits:limits(),session_start_equity:1000.0,peak_equity:1100.0,last_equity:1090.0,halt:None,observed_at_ms:1}; assert!(base.validate().is_ok()); assert!(RiskStateSnapshot{schema_version:99,..base}.validate().is_err()); assert!(RiskStateSnapshot{peak_equity:900.0,..base}.validate().is_err()); assert!(RiskStateSnapshot{last_equity:f64::NAN,..base}.validate().is_err()); assert!(RiskStateSnapshot{observed_at_ms:0,..base}.validate().is_err()); assert!(RiskStateSnapshot{peak_equity:1100.0,last_equity:1060.0,halt:None,..base}.validate().is_err()); }
    #[test] fn rebuilt_history_rejects_corrupt_observations() { assert!(evaluate_equity_history(1000.0,[1010.0,f64::NAN],1005.0,limits()).is_err()); assert!(evaluate_equity_history(1000.0,[1010.0],0.0,limits()).is_err()); }
    #[test] fn rejects_invalid_limits_and_equity() { assert!(SessionRiskCircuitBreaker::new(0.0,limits()).is_err()); assert!(SessionRiskCircuitBreaker::new(f64::NAN,limits()).is_err()); assert!(SessionRiskCircuitBreaker::new(1000.0,RiskLimits{max_daily_loss_fraction:1.0,max_drawdown_fraction:0.03}).is_err()); let mut g=SessionRiskCircuitBreaker::new(1000.0,limits()).unwrap(); assert!(g.observe_equity(f64::INFINITY).is_err()); }
}
