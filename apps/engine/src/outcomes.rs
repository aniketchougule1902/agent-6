use crate::types::Candle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    TakeProfit,
    StopLoss,
    Timeout,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeLabel {
    pub mfe_bps: f64,
    pub mae_bps: f64,
    pub time_to_target_ms: Option<u64>,
    pub time_to_stop_ms: Option<u64>,
    pub exit_reason: ExitReason,
    pub exit_ts_ms: u64,
}

/// Deterministically labels a long/short setup from completed candles only.
/// When TP and SL are both touched inside the same candle, the conservative
/// assumption is used: the stop is considered hit first. This avoids injecting
/// optimistic, unknowable intrabar ordering into research labels.
pub fn label_outcome(
    entry: f64,
    stop: f64,
    target: f64,
    side: i8,
    entry_ts_ms: u64,
    horizon_ms: u64,
    candles: &[Candle],
) -> Option<OutcomeLabel> {
    if entry <= 0.0 || !matches!(side, -1 | 1) || horizon_ms == 0 {
        return None;
    }

    let horizon_end = entry_ts_ms.saturating_add(horizon_ms);
    let mut mfe_bps: f64 = 0.0;
    let mut mae_bps: f64 = 0.0;
    let mut target_ts = None;
    let mut stop_ts = None;
    let mut last_ts = entry_ts_ms;

    for candle in candles.iter().filter(|c| c.confirmed && c.start_ms >= entry_ts_ms && c.start_ms <= horizon_end) {
        last_ts = candle.end_ms.max(candle.start_ms);
        let (favorable, adverse, hit_target, hit_stop) = if side > 0 {
            (
                (candle.high - entry) / entry * 10_000.0,
                (entry - candle.low) / entry * 10_000.0,
                candle.high >= target,
                candle.low <= stop,
            )
        } else {
            (
                (entry - candle.low) / entry * 10_000.0,
                (candle.high - entry) / entry * 10_000.0,
                candle.low <= target,
                candle.high >= stop,
            )
        };
        mfe_bps = mfe_bps.max(favorable.max(0.0));
        mae_bps = mae_bps.max(adverse.max(0.0));

        if hit_target && target_ts.is_none() {
            target_ts = Some(last_ts);
        }
        if hit_stop && stop_ts.is_none() {
            stop_ts = Some(last_ts);
        }

        // Same-bar ordering is unknowable from OHLC; choose the adverse path.
        if hit_stop {
            return Some(OutcomeLabel {
                mfe_bps,
                mae_bps,
                time_to_target_ms: target_ts.map(|ts| ts.saturating_sub(entry_ts_ms)),
                time_to_stop_ms: stop_ts.map(|ts| ts.saturating_sub(entry_ts_ms)),
                exit_reason: ExitReason::StopLoss,
                exit_ts_ms: last_ts,
            });
        }
        if hit_target {
            return Some(OutcomeLabel {
                mfe_bps,
                mae_bps,
                time_to_target_ms: target_ts.map(|ts| ts.saturating_sub(entry_ts_ms)),
                time_to_stop_ms: None,
                exit_reason: ExitReason::TakeProfit,
                exit_ts_ms: last_ts,
            });
        }
    }

    Some(OutcomeLabel {
        mfe_bps,
        mae_bps,
        time_to_target_ms: target_ts.map(|ts| ts.saturating_sub(entry_ts_ms)),
        time_to_stop_ms: stop_ts.map(|ts| ts.saturating_sub(entry_ts_ms)),
        exit_reason: ExitReason::Timeout,
        exit_ts_ms: last_ts.min(horizon_end),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(start_ms: u64, high: f64, low: f64) -> Candle {
        Candle {
            start_ms,
            end_ms: start_ms + 60_000,
            interval: "1".into(),
            open: 100.0,
            high,
            low,
            close: 100.0,
            volume: 1.0,
            turnover: 100.0,
            confirmed: true,
        }
    }

    #[test]
    fn long_target_label_tracks_excursions_and_time() {
        let label = label_outcome(100.0, 98.0, 103.0, 1, 1_000, 180_000, &[
            candle(1_000, 101.0, 99.0),
            candle(61_000, 103.5, 99.5),
        ]).unwrap();
        assert_eq!(label.exit_reason, ExitReason::TakeProfit);
        assert_eq!(label.time_to_target_ms, Some(120_000));
        assert!((label.mfe_bps - 350.0).abs() < 1e-9);
        assert!((label.mae_bps - 100.0).abs() < 1e-9);
    }

    #[test]
    fn same_bar_tp_and_sl_uses_conservative_stop_first_rule() {
        let label = label_outcome(100.0, 98.0, 102.0, 1, 1_000, 60_000, &[
            candle(1_000, 103.0, 97.0),
        ]).unwrap();
        assert_eq!(label.exit_reason, ExitReason::StopLoss);
        assert_eq!(label.time_to_stop_ms, Some(60_000));
    }

    #[test]
    fn short_timeout_ignores_unconfirmed_future_information() {
        let mut future = candle(121_000, 90.0, 80.0);
        future.confirmed = false;
        let label = label_outcome(100.0, 102.0, 95.0, -1, 1_000, 180_000, &[
            candle(1_000, 100.5, 99.0),
            future,
        ]).unwrap();
        assert_eq!(label.exit_reason, ExitReason::Timeout);
        assert!((label.mfe_bps - 100.0).abs() < 1e-9);
    }
}