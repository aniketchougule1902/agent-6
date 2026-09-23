use crate::{
    bybit_record::normalize_message,
    replay::{MarketRecorder, NormalizedMarketEvent},
};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default)]
struct BookCursor {
    seen_snapshot: bool,
    last_seq: u64,
    last_update_id: u64,
}

/// Stateful admission gate for the live normalized recorder.
///
/// The gate deliberately mirrors the replay integrity contract at ingestion time:
/// an L50 delta is never persisted before a snapshot, and stale/non-monotonic
/// sequence/update IDs are rejected instead of poisoning a deterministic recording.
#[derive(Debug, Clone, Default)]
pub struct RecordingIntegrityGate {
    books: HashMap<String, BookCursor>,
}

impl RecordingIntegrityGate {
    pub fn accept(&mut self, event: &NormalizedMarketEvent) -> Result<()> {
        let NormalizedMarketEvent::OrderBook {
            symbol,
            update_id,
            seq,
            snapshot,
            ..
        } = event
        else {
            return Ok(());
        };

        let cursor = self.books.entry(symbol.clone()).or_default();
        if *snapshot || *update_id == 1 {
            cursor.seen_snapshot = true;
            cursor.last_seq = *seq;
            cursor.last_update_id = *update_id;
            return Ok(());
        }

        if !cursor.seen_snapshot {
            bail!("refusing to record {symbol} L50 delta before snapshot");
        }
        if *seq > 0 && cursor.last_seq > 0 && *seq <= cursor.last_seq {
            bail!("refusing stale {symbol} L50 seq {seq} <= {}", cursor.last_seq);
        }
        if *update_id > 0 && cursor.last_update_id > 0 && *update_id <= cursor.last_update_id {
            bail!(
                "refusing stale {symbol} L50 update id {update_id} <= {}",
                cursor.last_update_id
            );
        }

        cursor.last_seq = *seq;
        cursor.last_update_id = *update_id;
        Ok(())
    }

    pub fn reset_symbol(&mut self, symbol: &str) {
        self.books.remove(symbol);
    }
}

/// Recorder facade used by the live feed. Admission is checked before disk I/O,
/// so a rejected event cannot appear in the JSONL recording.
pub struct AcceptedMarketRecorder {
    recorder: MarketRecorder,
    gate: RecordingIntegrityGate,
}

impl AcceptedMarketRecorder {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            recorder: MarketRecorder::open(path)?,
            gate: RecordingIntegrityGate::default(),
        })
    }

    pub fn append(&mut self, event: &NormalizedMarketEvent) -> Result<()> {
        self.gate.accept(event)?;
        self.recorder.append(event)
    }

    /// Normalize and admit one complete Bybit websocket payload as a batch.
    ///
    /// Validation runs against a cloned gate first, so if any event in the payload
    /// violates feed integrity, none of that payload is written and the live gate
    /// cursor is unchanged. This is especially important for batched exchange data.
    pub fn append_bybit_message(&mut self, text: &str) -> Result<usize> {
        let events = normalize_message(text);
        if events.is_empty() {
            return Ok(0);
        }

        let mut candidate_gate = self.gate.clone();
        for event in &events {
            candidate_gate.accept(event)?;
        }
        for event in &events {
            self.recorder.append(event)?;
        }
        self.gate = candidate_gate;
        Ok(events.len())
    }

    pub fn reset_symbol(&mut self, symbol: &str) {
        self.gate.reset_symbol(symbol);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::read_recording;
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    fn book(seq: u64, update_id: u64, snapshot: bool) -> NormalizedMarketEvent {
        NormalizedMarketEvent::OrderBook {
            ts_ms: seq,
            symbol: "BTCUSDT".into(),
            update_id,
            seq,
            snapshot,
            bids: vec![(100.0, 1.0)],
            asks: vec![(101.0, 1.0)],
        }
    }

    fn temp_recording_path() -> std::path::PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("agent6-live-recorder-{nonce}.jsonl"))
    }

    fn bybit_book(kind: &str, seq: u64, update_id: u64) -> String {
        format!(
            r#"{{"topic":"orderbook.50.BTCUSDT","type":"{kind}","ts":{seq},"data":{{"s":"BTCUSDT","b":[["100","1"]],"a":[["101","1"]],"u":{update_id},"seq":{seq},"cts":{seq}}}}}"#
        )
    }

    #[test]
    fn rejects_delta_before_snapshot() {
        let mut gate = RecordingIntegrityGate::default();
        assert!(gate.accept(&book(2, 2, false)).is_err());
    }

    #[test]
    fn accepts_monotonic_snapshot_delta_stream() {
        let mut gate = RecordingIntegrityGate::default();
        gate.accept(&book(10, 100, true)).unwrap();
        gate.accept(&book(11, 101, false)).unwrap();
        gate.accept(&book(12, 102, false)).unwrap();
    }

    #[test]
    fn rejects_stale_seq_and_update_id() {
        let mut gate = RecordingIntegrityGate::default();
        gate.accept(&book(10, 100, true)).unwrap();
        assert!(gate.accept(&book(10, 101, false)).is_err());

        let mut gate = RecordingIntegrityGate::default();
        gate.accept(&book(10, 100, true)).unwrap();
        assert!(gate.accept(&book(11, 100, false)).is_err());
    }

    #[test]
    fn snapshot_resets_cursor_after_reconnect() {
        let mut gate = RecordingIntegrityGate::default();
        gate.accept(&book(50, 500, true)).unwrap();
        gate.accept(&book(51, 501, false)).unwrap();
        gate.accept(&book(1, 1, true)).unwrap();
        gate.accept(&book(2, 2, false)).unwrap();
    }

    #[test]
    fn bybit_batch_admission_rejects_bad_delta_without_advancing_gate() {
        let mut gate = RecordingIntegrityGate::default();
        gate.accept(&book(10, 100, true)).unwrap();
        let before = gate.clone();
        assert!(gate.accept(&book(10, 101, false)).is_err());
        let mut restored = before;
        assert!(restored.accept(&book(11, 101, false)).is_ok());
    }

    #[test]
    fn rejected_bybit_l50_payload_is_zero_write_and_does_not_advance_cursor() {
        let path = temp_recording_path();
        let mut recorder = AcceptedMarketRecorder::open(&path).unwrap();

        assert_eq!(recorder.append_bybit_message(&bybit_book("snapshot", 10, 100)).unwrap(), 1);
        let bytes_after_snapshot = fs::metadata(&path).unwrap().len();

        assert!(recorder.append_bybit_message(&bybit_book("delta", 10, 101)).is_err());
        assert_eq!(fs::metadata(&path).unwrap().len(), bytes_after_snapshot);

        assert_eq!(recorder.append_bybit_message(&bybit_book("delta", 11, 101)).unwrap(), 1);
        let events = read_recording(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], NormalizedMarketEvent::OrderBook { snapshot: true, seq: 10, update_id: 100, .. }));
        assert!(matches!(events[1], NormalizedMarketEvent::OrderBook { snapshot: false, seq: 11, update_id: 101, .. }));

        fs::remove_file(path).unwrap();
    }
}