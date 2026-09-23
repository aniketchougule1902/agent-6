use crate::types::{ChartFlag, EngineEvent};
use std::{collections::VecDeque,fs::File,io::{BufRead,BufReader,Seek,SeekFrom},path::Path};

pub fn from_event(event: &EngineEvent) -> Option<ChartFlag> {
    if !["signal","tp1","tp2","stop_loss","expired","reversed","invalidated"].contains(&event.event_type.as_str()) {return None;}
    let signal=event.signal.as_ref()?;
    let price=if event.event_type=="signal" {(signal.entry_low+signal.entry_high)/2.0} else {signal.observed_exit_price?};
    Some(ChartFlag {id:format!("{}:{}",signal.id,event.event_type),symbol:signal.symbol.clone(),timeframe:signal.timeframe.clone(),ts_ms:event.ts_ms,price,side:signal.side.clone(),kind:event.event_type.clone()})
}

pub fn append(flags: &mut VecDeque<ChartFlag>, flag: ChartFlag) {
    if flags.iter().any(|old| old.id==flag.id) {return;}
    flags.push_back(flag);
    while flags.len()>1000 {flags.pop_front();}
}

/// Read only the tail; old journals lacking observed exit prices are not
/// converted into invented fill/exit flags.
pub fn load(path:&Path) -> VecDeque<ChartFlag> {
    let mut flags=VecDeque::new();
    let Ok(mut file)=File::open(path) else {return flags;};
    let offset=file.metadata().map(|m| m.len().saturating_sub(2_000_000)).unwrap_or(0);
    if file.seek(SeekFrom::Start(offset)).is_err() {return flags;}
    let mut reader=BufReader::new(file);
    if offset>0 {let _=reader.read_line(&mut String::new());}
    for line in reader.lines().map_while(Result::ok) {
        if let Ok(event)=serde_json::from_str::<EngineEvent>(&line) {
            if let Some(flag)=from_event(&event) {append(&mut flags,flag);}
        }
    }
    flags
}
