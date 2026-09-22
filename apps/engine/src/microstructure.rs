#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct DepthDynamics {
    pub bid_slope: f64,
    pub ask_slope: f64,
    /// Positive means net bid replenishment / ask depletion; negative is the reverse.
    pub pressure: f64,
}

/// Computes normalized depth shape plus update-to-update replenishment/depletion pressure.
/// Levels must be ordered best-to-worst on each side.
pub(crate) fn depth_dynamics(
    bids: &[(f64, f64)],
    asks: &[(f64, f64)],
    previous_bid_depth: f64,
    previous_ask_depth: f64,
) -> DepthDynamics {
    let bid_depth: f64 = bids.iter().map(|(_, qty)| qty.max(0.0)).sum();
    let ask_depth: f64 = asks.iter().map(|(_, qty)| qty.max(0.0)).sum();

    let bid_delta = relative_delta(previous_bid_depth, bid_depth);
    let ask_delta = relative_delta(previous_ask_depth, ask_depth);

    DepthDynamics {
        bid_slope: side_slope(bids),
        ask_slope: side_slope(asks),
        pressure: ((bid_delta - ask_delta) / 2.0).clamp(-1.0, 1.0),
    }
}

/// Measures whether liquidity is concentrated near touch (+) or deeper in book (-).
/// Quantity is normalized so this remains comparable across symbols and price levels.
fn side_slope(levels: &[(f64, f64)]) -> f64 {
    if levels.len() < 2 {
        return 0.0;
    }
    let total: f64 = levels.iter().map(|(_, qty)| qty.max(0.0)).sum();
    if total <= f64::EPSILON {
        return 0.0;
    }

    let n = levels.len() as f64;
    let weighted_rank: f64 = levels
        .iter()
        .enumerate()
        .map(|(rank, (_, qty))| (rank as f64 / (n - 1.0)) * qty.max(0.0))
        .sum::<f64>()
        / total;

    (1.0 - 2.0 * weighted_rank).clamp(-1.0, 1.0)
}

fn relative_delta(previous: f64, current: f64) -> f64 {
    if previous <= f64::EPSILON {
        return 0.0;
    }
    ((current - previous) / previous).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slope_is_positive_when_depth_is_near_touch() {
        let near = vec![(100.0, 10.0), (99.0, 3.0), (98.0, 1.0)];
        let deep = vec![(101.0, 1.0), (102.0, 3.0), (103.0, 10.0)];
        assert!(side_slope(&near) > 0.0);
        assert!(side_slope(&deep) < 0.0);
    }

    #[test]
    fn pressure_detects_bid_replenishment_and_ask_depletion() {
        let bids = vec![(100.0, 12.0), (99.0, 8.0)];
        let asks = vec![(101.0, 4.0), (102.0, 4.0)];
        let result = depth_dynamics(&bids, &asks, 10.0, 10.0);
        assert!(result.pressure > 0.0);
    }

    #[test]
    fn empty_book_is_neutral() {
        assert_eq!(depth_dynamics(&[], &[], 0.0, 0.0), DepthDynamics::default());
    }
}
