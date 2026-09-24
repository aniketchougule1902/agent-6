use crate::types::{Candle, TimeframeAnalysis};
use std::collections::VecDeque;

pub const TIMEFRAMES: [&str; 4] = ["1", "3", "5", "15"];

fn ema_series(values: &[f64], period: usize) -> Vec<f64> {
    if values.len() < period { return vec![]; }
    let mut current = values[..period].iter().sum::<f64>() / period as f64;
    let mut out = vec![current];
    let alpha = 2.0 / (period as f64 + 1.0);
    for &value in &values[period..] {
        current += alpha * (value - current);
        out.push(current);
    }
    out
}

fn rma(values: &[f64], period: usize) -> Vec<f64> {
    if values.len() < period { return vec![]; }
    let mut current = values[..period].iter().sum::<f64>() / period as f64;
    let mut out = vec![current];
    for &value in &values[period..] {
        current = (current * (period - 1) as f64 + value) / period as f64;
        out.push(current);
    }
    out
}

/// Decisions use fully closed candles only. The wall-clock closure test also
/// protects against incorrectly marked REST candles and future timestamps.
pub fn analyze(bars: &VecDeque<Candle>, timeframe: &str, now: u64) -> Option<TimeframeAnalysis> {
    let duration = timeframe.parse::<u64>().ok()? * 60_000;
    let closed: Vec<_> = bars.iter().filter(|b| b.start_ms.saturating_add(duration) <= now).collect();
    if closed.len() < 60 { return None; }
    if closed.iter().any(|b| {
        ![b.open,b.high,b.low,b.close,b.volume].iter().all(|v| v.is_finite())
        || b.low <= 0.0 || b.volume < 0.0 || b.high < b.open.max(b.close)
        || b.low > b.open.min(b.close)
    }) { return None; }
    let last = *closed.last()?;
    let prices: Vec<_> = closed.iter().map(|b| b.close).collect();
    let ema9 = *ema_series(&prices,9).last()?;
    let ema21 = *ema_series(&prices,21).last()?;
    let ema50 = *ema_series(&prices,50).last()?;
    let changes: Vec<_> = prices.windows(2).map(|w| w[1]-w[0]).collect();
    let gains: Vec<_> = changes.iter().map(|x| x.max(0.0)).collect();
    let losses: Vec<_> = changes.iter().map(|x| (-x).max(0.0)).collect();
    let gain = *rma(&gains,14).last()?;
    let loss = *rma(&losses,14).last()?;
    let rsi14 = if gain + loss <= f64::EPSILON { 50.0 } else { 100.0 * gain / (gain+loss) };
    let mut tr = vec![];
    let mut plus = vec![];
    let mut minus = vec![];
    for pair in closed.windows(2) {
        let (a,b) = (pair[0],pair[1]);
        tr.push((b.high-b.low).max((b.high-a.close).abs()).max((b.low-a.close).abs()));
        let up = b.high-a.high;
        let down = a.low-b.low;
        plus.push(if up > down && up > 0.0 {up} else {0.0});
        minus.push(if down > up && down > 0.0 {down} else {0.0});
    }
    let atrs = rma(&tr,14);
    let atr14 = *atrs.last()?;
    let plus = rma(&plus,14);
    let minus = rma(&minus,14);
    let dx: Vec<_> = plus.iter().zip(&minus).map(|(p,m)| if p+m > 0.0 {100.0*(p-m).abs()/(p+m)} else {0.0}).collect();
    let adx14 = *rma(&dx,14).last()?;
    let fast = ema_series(&prices,12);
    let slow = ema_series(&prices,26);
    let macd: Vec<_> = fast[14..].iter().zip(&slow).map(|(f,s)| f-s).collect();
    let macd_histogram = macd.last()? - ema_series(&macd,9).last()?;
    let recent = &closed[closed.len()-20..];
    let mean = recent.iter().map(|b| b.close).sum::<f64>()/20.0;
    let deviation = (recent.iter().map(|b| (b.close-mean).powi(2)).sum::<f64>()/20.0).sqrt();
    let volume = recent.iter().map(|b| b.volume).sum::<f64>();
    if atr14 <= 0.0 || volume <= 0.0 { return None; }
    let vwap20 = recent.iter().map(|b| (b.high+b.low+b.close)/3.0*b.volume).sum::<f64>()/volume;
    let prior = &closed[closed.len()-21..closed.len()-1];
    let baseline_volume = prior.iter().map(|b| b.volume).sum::<f64>()/20.0;
    let relative_volume = if baseline_volume > 0.0 {last.volume/baseline_volume} else {0.0};
    let support = prior.iter().map(|b| b.low).fold(f64::INFINITY,f64::min);
    let resistance = prior.iter().map(|b| b.high).fold(f64::NEG_INFINITY,f64::max);
    let direction = if ema9 > ema21 && ema21 > ema50 {1.0} else if ema9 < ema21 && ema21 < ema50 {-1.0} else {0.0};
    let breakout = direction > 0.0 && last.close > resistance || direction < 0.0 && last.close < support;
    let pullback = direction > 0.0 && last.low <= ema21 + 0.3*atr14 && last.close > ema21 && last.close > last.open
        || direction < 0.0 && last.high >= ema21-0.3*atr14 && last.close < ema21 && last.close < last.open;
    let setup = if breakout {"channel_breakout"} else if pullback {"trend_pullback"} else {"waiting"};
    let candle_range = (last.high-last.low).max(f64::EPSILON);
    let candle_body = (last.close-last.open).abs();
    let body_fraction = (candle_body/candle_range).clamp(0.0,1.0);
    let directional_close_location = if direction > 0.0 {
        ((last.close-last.low)/candle_range).clamp(0.0,1.0)
    } else if direction < 0.0 {
        ((last.high-last.close)/candle_range).clamp(0.0,1.0)
    } else {0.0};
    let breakout_margin_atr = if breakout {
        if direction > 0.0 {(last.close-resistance)/atr14} else {(support-last.close)/atr14}
    } else {0.0};
    let range_atr = candle_range/atr14;
    let mut blockers = vec![];
    if now.saturating_sub(last.start_ms+duration) > duration {blockers.push("Candle history is stale".into());}
    if closed[closed.len()-60..].windows(2).any(|w| w[1].start_ms != w[0].start_ms+duration) {blockers.push("Gap in recent candle history".into());}
    if direction == 0.0 {blockers.push("EMA trend is mixed".into());}
    if adx14 < 20.0 {blockers.push("Trend strength below ADX 20".into());}
    if setup == "waiting" {blockers.push("Waiting for a closed-candle pullback or breakout".into());}
    if direction*macd_histogram <= 0.0 {blockers.push("MACD momentum disagrees".into());}
    if direction > 0.0 && !(45.0..=72.0).contains(&rsi14) || direction < 0.0 && !(28.0..=55.0).contains(&rsi14) {blockers.push("RSI outside continuation range".into());}
    if relative_volume < if breakout {1.2} else {0.7} {blockers.push("Insufficient relative volume".into());}
    if direction*(last.close-vwap20) < 0.0 {blockers.push("Price disagrees with rolling VWAP".into());}
    if breakout {
        if breakout_margin_atr < 0.08 {blockers.push("Breakout close is too close to the old channel; fakeout risk".into());}
        if breakout_margin_atr > 1.10 {blockers.push("Breakout is overextended beyond the entry window".into());}
        if body_fraction < 0.45 {blockers.push("Breakout candle body is weak versus its wick range".into());}
        if directional_close_location < 0.68 {blockers.push("Breakout candle shows rejection near the close".into());}
        if range_atr > 2.0 {blockers.push("Breakout candle is an exhaustion-sized range".into());}
    }
    let trend = if direction != 0.0 {((adx14-18.0)/22.0).clamp(0.0,1.0)} else {0.0};
    let momentum = if direction*macd_histogram > 0.0 {
        ((macd_histogram.abs()/atr14)/0.18).clamp(0.0,1.0)
    } else {0.0};
    let structure = if breakout {
        let margin_score = (breakout_margin_atr/0.35).clamp(0.0,1.0)
            * (1.0-(breakout_margin_atr-0.55).max(0.0)/0.55).clamp(0.0,1.0);
        (0.45*margin_score + 0.30*body_fraction + 0.25*directional_close_location).clamp(0.0,1.0)
    } else if pullback {
        (0.55*body_fraction + 0.45*directional_close_location).clamp(0.0,1.0)
    } else {0.0};
    let participation = (relative_volume/1.8).clamp(0.0,1.0);
    Some(TimeframeAnalysis {
        timeframe: timeframe.into(), candle_ms:last.start_ms, close:last.close,
        ema9,ema21,ema50,rsi14,adx14,macd_histogram,atr14,vwap20,
        bb_upper:mean+2.0*deviation,bb_lower:mean-2.0*deviation,relative_volume,support,resistance,
        bias: if direction>0.0 {"long"} else if direction<0.0 {"short"} else {"neutral"}.into(),
        setup:setup.into(), quality:(0.30*trend+0.25*momentum+0.30*structure+0.15*participation).clamp(0.0,1.0),blockers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bars() -> VecDeque<Candle> {
        (0..100).map(|i| { let p=100.0+i as f64; Candle {start_ms:i*60_000,end_ms:(i+1)*60_000-1,interval:"1".into(),open:p,high:p+1.0,low:p-1.0,close:p+0.5,volume:10.0,turnover:1000.0,confirmed:true} }).collect()
    }
    #[test]
    fn future_and_open_candles_cannot_change_analysis() {
        let mut data=bars();
        let before=analyze(&data,"1",6_000_000).unwrap();
        let mut future=data.back().unwrap().clone(); future.start_ms=6_000_000; future.close=9000.0; future.high=9000.0;
        data.push_back(future);
        let after=analyze(&data,"1",6_000_000).unwrap();
        assert_eq!(before.ema21,after.ema21); assert_eq!(before.rsi14,after.rsi14); assert_eq!(before.candle_ms,after.candle_ms);
    }
    #[test]
    fn wilder_extremes_and_channel_excludes_signal_candle() {
        let a=analyze(&bars(),"1",6_000_000).unwrap();
        assert!((a.rsi14-100.0).abs()<1e-8); assert!((a.adx14-100.0).abs()<1e-8);
        assert_eq!(a.resistance,199.0); assert!(a.atr14.is_finite());
        assert!(a.blockers.iter().any(|b| b.contains("weak versus its wick range")));
    }
    #[test]
    fn bad_data_fails_closed_and_gaps_are_blocked() {
        let mut data=bars(); data[20].close=f64::NAN; assert!(analyze(&data,"1",6_000_000).is_none());
        let mut data=bars(); data.remove(80); assert!(analyze(&data,"1",6_000_000).unwrap().blockers.iter().any(|b| b.contains("Gap")));
    }
}
