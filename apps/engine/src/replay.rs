use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{create_dir_all, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NormalizedMarketEvent {
    Trade {
        ts_ms: u64,
        symbol: String,
        price: f64,
        qty: f64,
        side: String,
    },
    OrderBook {
        ts_ms: u64,
        symbol: String,
        update_id: u64,
        seq: u64,
        snapshot: bool,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    },
    Ticker {
        ts_ms: u64,
        symbol: String,
        last_price: Option<f64>,
        mark_price: Option<f64>,
        index_price: Option<f64>,
        open_interest: Option<f64>,
        funding_rate: Option<f64>,
    },
    Liquidation {
        ts_ms: u64,
        symbol: String,
        price: f64,
        qty: f64,
        side: String,
    },
    Kline {
        ts_ms: u64,
        symbol: String,
        interval: String,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
        turnover: f64,
        confirmed: bool,
    },
}

impl NormalizedMarketEvent {
    pub fn ts_ms(&self) -> u64 {
        match self {
            Self::Trade { ts_ms, .. }
            | Self::OrderBook { ts_ms, .. }
            | Self::Ticker { ts_ms, .. }
            | Self::Liquidation { ts_ms, .. }
            | Self::Kline { ts_ms, .. } => *ts_ms,
        }
    }
}

pub struct MarketRecorder {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl MarketRecorder {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            create_dir_all(parent).with_context(|| format!("create recorder directory {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open market recorder {}", path.display()))?;
        Ok(Self { path, writer: BufWriter::new(file) })
    }

    pub fn append(&mut self, event: &NormalizedMarketEvent) -> Result<()> {
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn read_recording(path: impl AsRef<Path>) -> Result<Vec<NormalizedMarketEvent>> {
    let file = File::open(path.as_ref())
        .with_context(|| format!("open replay recording {}", path.as_ref().display()))?;
    let mut events = Vec::new();
    for (line_no, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str(&line)
            .with_context(|| format!("decode normalized market event at line {}", line_no + 1))?;
        events.push(event);
    }
    Ok(events)
}

#[derive(Debug, Clone)]
pub struct ReplayFrame {
    pub replay_ts_ms: u64,
    pub elapsed_ms: u64,
    pub event: NormalizedMarketEvent,
}

pub struct DeterministicReplay {
    events: Vec<NormalizedMarketEvent>,
    cursor: usize,
    origin_ms: u64,
}

impl DeterministicReplay {
    pub fn new(mut events: Vec<NormalizedMarketEvent>) -> Self {
        events.sort_by_key(NormalizedMarketEvent::ts_ms);
        let origin_ms = events.first().map(NormalizedMarketEvent::ts_ms).unwrap_or_default();
        Self { events, cursor: 0, origin_ms }
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Iterator for DeterministicReplay {
    type Item = ReplayFrame;

    fn next(&mut self) -> Option<Self::Item> {
        let event = self.events.get(self.cursor)?.clone();
        self.cursor += 1;
        let replay_ts_ms = event.ts_ms();
        Some(ReplayFrame {
            replay_ts_ms,
            elapsed_ms: replay_ts_ms.saturating_sub(self.origin_ms),
            event,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(ts_ms: u64, price: f64) -> NormalizedMarketEvent {
        NormalizedMarketEvent::Trade {
            ts_ms,
            symbol: "BTCUSDT".into(),
            price,
            qty: 1.0,
            side: "buy".into(),
        }
    }

    #[test]
    fn replay_is_sorted_and_time_is_relative_to_first_event() {
        let frames: Vec<_> = DeterministicReplay::new(vec![trade(1_500, 2.0), trade(1_000, 1.0)])
            .collect();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].replay_ts_ms, 1_000);
        assert_eq!(frames[0].elapsed_ms, 0);
        assert_eq!(frames[1].replay_ts_ms, 1_500);
        assert_eq!(frames[1].elapsed_ms, 500);
    }

    #[test]
    fn reset_replays_identical_sequence() {
        let mut replay = DeterministicReplay::new(vec![trade(10, 1.0), trade(20, 2.0)]);
        let first: Vec<_> = replay.by_ref().map(|frame| frame.replay_ts_ms).collect();
        replay.reset();
        let second: Vec<_> = replay.by_ref().map(|frame| frame.replay_ts_ms).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn json_round_trip_preserves_typed_event() {
        let event = NormalizedMarketEvent::OrderBook {
            ts_ms: 42,
            symbol: "ETHUSDT".into(),
            update_id: 7,
            seq: 9,
            snapshot: true,
            bids: vec![(100.0, 2.0)],
            asks: vec![(101.0, 3.0)],
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: NormalizedMarketEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }
}