use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A clock-normalized, already-admitted top-of-book observation used only for
/// asynchronous research/explanation context. It is intentionally not wired
/// into the deterministic Bybit signal hot path.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VenueObservation {
    pub normalized_ms: u64,
    pub bid: f64,
    pub ask: f64,
    /// Signed aggressive-flow notional over the observation interval.
    pub signed_flow: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CrossVenueContext {
    pub as_of_ms: u64,
    pub secondary_age_ms: u64,
    pub mid_spread_bps: f64,
    pub return_lead_bps: Option<f64>,
    pub flow_divergence: Option<f64>,
    pub samples: usize,
}

/// Bounded causal join between the primary Bybit observation and secondary
/// venue observations. `context_at(T)` may use only secondary observations with
/// timestamp <= T. Future samples are rejected from the join rather than
/// backfilled, which makes replay prefix-invariant.
#[derive(Debug, Clone)]
pub struct CausalVenueContext {
    secondary: VecDeque<VenueObservation>,
    capacity: usize,
    max_age_ms: u64,
}

impl CausalVenueContext {
    pub fn new(capacity: usize, max_age_ms: u64) -> Result<Self> {
        ensure!((2..=4096).contains(&capacity), "venue context capacity must be in 2..=4096");
        ensure!(max_age_ms > 0, "venue context max age must be positive");
        Ok(Self { secondary: VecDeque::with_capacity(capacity), capacity, max_age_ms })
    }

    pub fn push_secondary(&mut self, observation: VenueObservation) -> Result<()> {
        validate_observation(observation)?;
        if let Some(last) = self.secondary.back() {
            ensure!(observation.normalized_ms > last.normalized_ms, "secondary venue timestamp must increase strictly");
        }
        self.secondary.push_back(observation);
        if self.secondary.len() > self.capacity { self.secondary.pop_front(); }
        Ok(())
    }

    pub fn context_at(&self, primary: VenueObservation) -> Result<CrossVenueContext> {
        validate_observation(primary)?;
        let current_index = self.secondary.iter().rposition(|s| s.normalized_ms <= primary.normalized_ms)
            .ok_or_else(|| anyhow::anyhow!("no causal secondary observation available"))?;
        let current = self.secondary[current_index];
        let age = primary.normalized_ms - current.normalized_ms;
        ensure!(age <= self.max_age_ms, "secondary venue context is stale");

        let primary_mid = midpoint(primary)?;
        let secondary_mid = midpoint(current)?;
        let mid_spread_bps = (secondary_mid / primary_mid - 1.0) * 10_000.0;
        ensure!(mid_spread_bps.is_finite(), "cross-venue spread is non-finite");

        let previous = current_index.checked_sub(1).map(|i| self.secondary[i]);
        let return_lead_bps = if let Some(previous) = previous {
            let previous_mid = midpoint(previous)?;
            let value = (secondary_mid / previous_mid - 1.0) * 10_000.0;
            ensure!(value.is_finite(), "secondary return is non-finite");
            Some(value)
        } else { None };

        let flow_divergence = previous.map(|_| {
            // Scale-free signed disagreement. Positive means the secondary flow
            // is more buy-heavy than the primary observation; no labels/future
            // outcomes participate in this calculation.
            let denom = current.signed_flow.abs() + primary.signed_flow.abs();
            if denom <= f64::EPSILON { 0.0 } else { (current.signed_flow - primary.signed_flow) / denom }
        });
        if let Some(value) = flow_divergence { ensure!(value.is_finite() && (-1.0..=1.0).contains(&value), "flow divergence is invalid"); }

        Ok(CrossVenueContext {
            as_of_ms: primary.normalized_ms,
            secondary_age_ms: age,
            mid_spread_bps,
            return_lead_bps,
            flow_divergence,
            samples: current_index + 1,
        })
    }
}

fn validate_observation(o: VenueObservation) -> Result<()> {
    ensure!(o.normalized_ms > 0, "venue observation timestamp is required");
    ensure!([o.bid, o.ask, o.signed_flow].iter().all(|v| v.is_finite()), "venue observation contains non-finite values");
    ensure!(o.bid > 0.0 && o.ask >= o.bid, "venue observation has invalid book");
    Ok(())
}
fn midpoint(o: VenueObservation) -> Result<f64> {
    let mid = (o.bid + o.ask) / 2.0;
    ensure!(mid.is_finite() && mid > 0.0, "venue midpoint is invalid");
    Ok(mid)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn obs(ts:u64, mid:f64, flow:f64)->VenueObservation { VenueObservation{normalized_ms:ts,bid:mid-0.5,ask:mid+0.5,signed_flow:flow} }

    #[test]
    fn joins_only_evidence_available_at_as_of_time() {
        let mut c=CausalVenueContext::new(8,500).unwrap();
        c.push_secondary(obs(1000,100.0,10.0)).unwrap();
        c.push_secondary(obs(1100,101.0,20.0)).unwrap();
        c.push_secondary(obs(1300,500.0,-999.0)).unwrap(); // future extreme
        let x=c.context_at(obs(1150,100.5,5.0)).unwrap();
        assert_eq!(x.secondary_age_ms,50);
        assert_eq!(x.samples,2);
        assert!((x.return_lead_bps.unwrap()-100.0).abs()<1e-9);
        assert!((x.flow_divergence.unwrap()-0.6).abs()<1e-12);
    }

    #[test]
    fn future_mutation_cannot_change_past_context() {
        let mut a=CausalVenueContext::new(8,500).unwrap();
        let mut b=CausalVenueContext::new(8,500).unwrap();
        for c in [&mut a,&mut b] { c.push_secondary(obs(1000,100.0,1.0)).unwrap(); c.push_secondary(obs(1100,101.0,2.0)).unwrap(); }
        a.push_secondary(obs(2000,102.0,3.0)).unwrap();
        b.push_secondary(obs(2000,9999.0,-1e9)).unwrap();
        let primary=obs(1150,100.5,1.5);
        assert_eq!(a.context_at(primary).unwrap(),b.context_at(primary).unwrap());
    }

    #[test]
    fn stale_future_out_of_order_and_corrupt_inputs_fail_closed() {
        let mut c=CausalVenueContext::new(4,100).unwrap();
        c.push_secondary(obs(1000,100.0,0.0)).unwrap();
        assert!(c.context_at(obs(1200,100.0,0.0)).is_err());
        assert!(c.context_at(obs(900,100.0,0.0)).is_err());
        assert!(c.push_secondary(obs(1000,101.0,0.0)).is_err());
        assert!(c.push_secondary(VenueObservation{normalized_ms:1100,bid:101.0,ask:100.0,signed_flow:0.0}).is_err());
        assert!(c.push_secondary(VenueObservation{normalized_ms:1100,bid:100.0,ask:101.0,signed_flow:f64::NAN}).is_err());
    }

    #[test]
    fn bounded_history_evicts_old_context_without_lookahead() {
        let mut c=CausalVenueContext::new(2,500).unwrap();
        c.push_secondary(obs(1000,100.0,1.0)).unwrap();
        c.push_secondary(obs(1100,101.0,2.0)).unwrap();
        c.push_secondary(obs(1200,102.0,3.0)).unwrap();
        assert!(c.context_at(obs(1050,100.0,0.0)).is_err());
        let x=c.context_at(obs(1250,102.0,1.0)).unwrap();
        assert_eq!(x.samples,2);
    }
}