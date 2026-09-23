use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskLimits {
    /// Positive fraction of session-start equity, e.g. 0.03 = 3%.
    pub max_daily_loss_fraction: f64,
    /// Positive fraction from the intraday equity peak.
    pub max_drawdown_fraction: f64,
}

impl RiskLimits {
    pub fn validate(self) -> Result<Self> {
        if !self.max_daily_loss_fraction.is_finite()
            || !(0.0..1.0).contains(&self.max_daily_loss_fraction)
        {
            bail!("max_daily_loss_fraction must be finite and in (0, 1)");
        }
        if !self.max_drawdown_fraction.is_finite()
            || !(0.0..1.0).contains(&self.max_drawdown_fraction)
        {
            bail!("max_drawdown_fraction must be finite and in (0, 1)");
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskHaltReason {
    DailyLoss,
    Drawdown,
}

/// Fail-closed session risk guard. Once tripped it is latched until an explicit
/// session reset; subsequent equity recovery cannot silently re-enable signals.
#[derive(Debug, Clone)]
pub struct SessionRiskCircuitBreaker {
    limits: RiskLimits,
    session_start_equity: f64,
    peak_equity: f64,
    halt: Option<RiskHaltReason>,
}

impl SessionRiskCircuitBreaker {
    pub fn new(session_start_equity: f64, limits: RiskLimits) -> Result<Self> {
        let limits = limits.validate()?;
        validate_equity(session_start_equity)?;
        Ok(Self {
            limits,
            session_start_equity,
            peak_equity: session_start_equity,
            halt: None,
        })
    }

    pub fn observe_equity(&mut self, equity: f64) -> Result<Option<RiskHaltReason>> {
        validate_equity(equity)?;
        if self.halt.is_some() {
            return Ok(self.halt);
        }

        self.peak_equity = self.peak_equity.max(equity);
        let daily_loss = (self.session_start_equity - equity) / self.session_start_equity;
        let drawdown = (self.peak_equity - equity) / self.peak_equity;

        self.halt = if daily_loss >= self.limits.max_daily_loss_fraction {
            Some(RiskHaltReason::DailyLoss)
        } else if drawdown >= self.limits.max_drawdown_fraction {
            Some(RiskHaltReason::Drawdown)
        } else {
            None
        };
        Ok(self.halt)
    }

    pub fn is_halted(&self) -> bool {
        self.halt.is_some()
    }
}

fn validate_equity(equity: f64) -> Result<()> {
    if !equity.is_finite() || equity <= 0.0 {
        bail!("equity must be finite and positive");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> RiskLimits {
        RiskLimits {
            max_daily_loss_fraction: 0.05,
            max_drawdown_fraction: 0.03,
        }
    }

    #[test]
    fn trips_daily_loss_past_threshold_and_latches() {
        let mut guard = SessionRiskCircuitBreaker::new(1000.0, limits()).unwrap();
        assert_eq!(guard.observe_equity(951.0).unwrap(), None);
        assert_eq!(
            guard.observe_equity(949.0).unwrap(),
            Some(RiskHaltReason::DailyLoss)
        );
        assert!(guard.is_halted());
        assert_eq!(
            guard.observe_equity(1100.0).unwrap(),
            Some(RiskHaltReason::DailyLoss)
        );
    }

    #[test]
    fn trips_drawdown_from_peak_before_daily_loss() {
        let mut guard = SessionRiskCircuitBreaker::new(1000.0, limits()).unwrap();
        assert_eq!(guard.observe_equity(1100.0).unwrap(), None);
        assert_eq!(
            guard.observe_equity(1066.0).unwrap(),
            Some(RiskHaltReason::Drawdown)
        );
    }

    #[test]
    fn rejects_invalid_limits_and_equity() {
        assert!(SessionRiskCircuitBreaker::new(0.0, limits()).is_err());
        assert!(SessionRiskCircuitBreaker::new(f64::NAN, limits()).is_err());
        assert!(SessionRiskCircuitBreaker::new(
            1000.0,
            RiskLimits {
                max_daily_loss_fraction: 1.0,
                max_drawdown_fraction: 0.03,
            }
        )
        .is_err());
        let mut guard = SessionRiskCircuitBreaker::new(1000.0, limits()).unwrap();
        assert!(guard.observe_equity(f64::INFINITY).is_err());
    }
}
