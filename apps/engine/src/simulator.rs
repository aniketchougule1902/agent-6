use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LiquidityRole {
    Maker,
    Taker,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FeeSchedule {
    pub maker_bps: f64,
    pub taker_bps: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SimulationConfig {
    pub fees: FeeSchedule,
    pub decision_latency_ms: u64,
    pub order_latency_ms: u64,
    pub base_slippage_bps: f64,
    pub impact_bps_per_book_fraction: f64,
    pub queue_ahead_fraction: f64,
    pub max_fill_fraction: f64,
    pub seed: u64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            fees: FeeSchedule { maker_bps: 2.0, taker_bps: 5.5 },
            decision_latency_ms: 35,
            order_latency_ms: 55,
            base_slippage_bps: 0.35,
            impact_bps_per_book_fraction: 4.0,
            queue_ahead_fraction: 0.35,
            max_fill_fraction: 1.0,
            seed: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FillRequest {
    pub side_sign: i8,
    pub quantity: f64,
    pub reference_price: f64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub visible_touch_quantity: f64,
    pub role: LiquidityRole,
    pub is_stop: bool,
    pub gap_price: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SimulatedFill {
    pub requested_quantity: f64,
    pub filled_quantity: f64,
    pub fill_price: f64,
    pub fee_quote: f64,
    pub slippage_bps: f64,
    pub total_latency_ms: u64,
    pub partial: bool,
}

pub fn simulate_fill(config: SimulationConfig, request: FillRequest) -> SimulatedFill {
    let side = if request.side_sign >= 0 { 1.0 } else { -1.0 };
    let spread = (request.best_ask - request.best_bid).max(0.0);
    let mid = ((request.best_ask + request.best_bid) * 0.5).max(f64::EPSILON);
    let half_spread_bps = spread / mid * 5_000.0;

    let touch = request.visible_touch_quantity.max(f64::EPSILON);
    let book_fraction = (request.quantity / touch).max(0.0);
    let impact_bps = config.base_slippage_bps
        + half_spread_bps
        + config.impact_bps_per_book_fraction * book_fraction.sqrt();

    let queue_penalty = if request.role == LiquidityRole::Maker {
        config.queue_ahead_fraction.clamp(0.0, 0.99)
    } else {
        0.0
    };
    let capacity = touch * (1.0 - queue_penalty) * config.max_fill_fraction.clamp(0.0, 1.0);
    let filled_quantity = request.quantity.max(0.0).min(capacity.max(0.0));

    let baseline = if request.is_stop {
        request.gap_price.unwrap_or(request.reference_price)
    } else if request.role == LiquidityRole::Taker {
        if side > 0.0 { request.best_ask } else { request.best_bid }
    } else {
        request.reference_price
    };
    let fill_price = baseline * (1.0 + side * impact_bps / 10_000.0);
    let fee_bps = match request.role {
        LiquidityRole::Maker => config.fees.maker_bps,
        LiquidityRole::Taker => config.fees.taker_bps,
    };
    let fee_quote = filled_quantity * fill_price * fee_bps / 10_000.0;

    SimulatedFill {
        requested_quantity: request.quantity,
        filled_quantity,
        fill_price,
        fee_quote,
        slippage_bps: side * (fill_price - request.reference_price) / request.reference_price.max(f64::EPSILON) * 10_000.0,
        total_latency_ms: config.decision_latency_ms.saturating_add(config.order_latency_ms),
        partial: filled_quantity + f64::EPSILON < request.quantity,
    }
}

pub fn stressed_config(base: SimulationConfig, scenario: u64) -> SimulationConfig {
    let noise = deterministic_unit(base.seed ^ scenario);
    let latency_multiplier = 1.0 + noise * 4.0;
    let slippage_multiplier = 1.0 + noise * 5.0;
    SimulationConfig {
        decision_latency_ms: (base.decision_latency_ms as f64 * latency_multiplier).round() as u64,
        order_latency_ms: (base.order_latency_ms as f64 * latency_multiplier).round() as u64,
        base_slippage_bps: base.base_slippage_bps * slippage_multiplier,
        impact_bps_per_book_fraction: base.impact_bps_per_book_fraction * slippage_multiplier,
        max_fill_fraction: (base.max_fill_fraction * (1.0 - noise * 0.65)).clamp(0.05, 1.0),
        ..base
    }
}

fn deterministic_unit(mut x: u64) -> f64 {
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    let value = x.wrapping_mul(0x2545F4914F6CDD1D);
    (value as f64) / (u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> FillRequest {
        FillRequest {
            side_sign: 1,
            quantity: 2.0,
            reference_price: 100.0,
            best_bid: 99.9,
            best_ask: 100.1,
            visible_touch_quantity: 1.0,
            role: LiquidityRole::Taker,
            is_stop: false,
            gap_price: None,
        }
    }

    #[test]
    fn fees_slippage_latency_and_partial_fills_are_applied() {
        let fill = simulate_fill(SimulationConfig::default(), request());
        assert!(fill.partial);
        assert_eq!(fill.filled_quantity, 1.0);
        assert!(fill.fill_price > 100.1);
        assert!(fill.fee_quote > 0.0);
        assert_eq!(fill.total_latency_ms, 90);
    }

    #[test]
    fn stop_gap_uses_worse_gap_baseline() {
        let mut req = request();
        req.quantity = 0.5;
        req.is_stop = true;
        req.gap_price = Some(102.0);
        let fill = simulate_fill(SimulationConfig::default(), req);
        assert!(fill.fill_price > 102.0);
    }

    #[test]
    fn maker_queue_reduces_available_fill() {
        let mut req = request();
        req.quantity = 1.0;
        req.role = LiquidityRole::Maker;
        let fill = simulate_fill(SimulationConfig::default(), req);
        assert!(fill.filled_quantity < 1.0);
    }

    #[test]
    fn stress_scenarios_are_seeded_and_reproducible() {
        let base = SimulationConfig::default();
        let a = stressed_config(base, 42);
        let b = stressed_config(base, 42);
        assert_eq!(a.order_latency_ms, b.order_latency_ms);
        assert_eq!(a.base_slippage_bps, b.base_slippage_bps);
        assert!(a.order_latency_ms >= base.order_latency_ms);
    }
}
