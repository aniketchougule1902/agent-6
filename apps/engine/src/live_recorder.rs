use crate::{
    bybit_record::normalize_message,
    replay::{MarketRecorder, NormalizedMarketEvent},
};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

    /// Start a fresh single-symbol replay session while preserving the previous
    /// active recording beside it. This prevents events from different symbols
    /// or process lifetimes being timestamp-sorted into one synthetic replay.
    pub fn start_session(path: impl AsRef<Path>, label: &str) -> Result<(Self, Option<PathBuf>)> {
        let path = path.as_ref();
        let archived = archive_existing(path, label)?;
        Ok((Self::open(path)?, archived))
    }

    pub fn append(&mut self, event: &NormalizedMarketEvent) -> Result<()> {
        self.gate.accept(event)?;
        self.recorder.append(event)
    }

    /// Admit one already-normalized exchange payload as an atomic batch.
    ///
    /// Validation runs against a cloned gate first, so if any event in the payload
    /// violates feed integrity, none of that payload is written and the live gate
    /// cursor is unchanged. The caller can then apply these exact same typed events
    /// to live state, which is also the deterministic replay boundary.
    pub fn append_batch(&mut self, events: &[NormalizedMarketEvent]) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        let mut candidate_gate = self.gate.clone();
        for event in events {
            candidate_gate.accept(event)?;
        }
        for event in events {
            self.recorder.append(event)?;
        }
        self.gate = candidate_gate;
        Ok(events.len())
    }

    /// Compatibility helper for tests/tools that still start from raw Bybit JSON.
    pub fn append_bybit_message(&mut self, text: &str) -> Result<usize> {
        let events = normalize_message(text);
        self.append_batch(&events)
    }

    pub fn reset_symbol(&mut self, symbol: &str) {
        self.gate.reset_symbol(symbol);
    }
}

fn archive_existing(path: &Path, label: &str) -> Result<Option<PathBuf>> {
    if !path.exists() || std::fs::metadata(path)?.len() == 0 {
        return Ok(None);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let stem = path.file_stem().and_then(|x| x.to_str()).unwrap_or("market-events");
    let extension = path.extension().and_then(|x| x.to_str()).unwrap_or("jsonl");
    let safe_label: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut archive = parent.join(format!("{stem}.session-{nonce}-{safe_label}.{extension}"));
    let mut suffix = 0_u32;
    while archive.exists() {
        suffix += 1;
        archive = parent.join(format!("{stem}.session-{nonce}-{safe_label}-{suffix}.{extension}"));
    }
    std::fs::rename(path, &archive)?;
    Ok(Some(archive))
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
    fn start_session_archives_previous_recording_and_opens_clean_file() {
        let path = temp_recording_path();
        fs::write(&path, b"old-session\n").unwrap();
        let (mut recorder, archived) = AcceptedMarketRecorder::start_session(&path, "BTCUSDT").unwrap();
        let archived = archived.expect("previous session should be archived");
        assert_eq!(fs::read(&archived).unwrap(), b"old-session\n");
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);

        recorder.append(&book(10, 100, true)).unwrap();
        assert!(fs::metadata(&path).unwrap().len() > 0);
        fs::remove_file(path).unwrap();
        fs::remove_file(archived).unwrap();
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