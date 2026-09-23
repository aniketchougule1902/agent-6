use serde::{Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
pub struct Instrument {
    pub symbol: String,
    pub base: String,
    pub quote: String,
    pub tick_size: String,
    pub min_qty: String,
    pub qty_step: String,
}
#[derive(Clone, Default, Serialize)]
pub struct Catalog {
    pub instruments: Vec<Instrument>,
    pub updated_at_ms: u64,
    pub error: Option<String>,
}
static INSTRUMENTS: OnceLock<parking_lot::RwLock<Vec<Instrument>>> = OnceLock::new();
pub fn tick_size(symbol:&str)->Option<f64> {
    INSTRUMENTS.get()?.read().iter().find(|i|i.symbol==symbol)?.tick_size.parse::<f64>().ok().filter(|v|v.is_finite()&&*v>0.0)
}
static CACHE: OnceLock<Mutex<Catalog>> = OnceLock::new();
pub async fn get(testnet: bool) -> Catalog {
    let mut cache = CACHE.get_or_init(|| Mutex::new(Catalog::default())).lock().await;
    let now=crate::state::now_ms();
    if cache.updated_at_ms>0 && now.saturating_sub(cache.updated_at_ms)<900_000 {return cache.clone();}
    match fetch(testnet).await {
        Ok(instruments)=>{
            *INSTRUMENTS.get_or_init(||parking_lot::RwLock::new(Vec::new())).write()=instruments.clone();
            *cache=Catalog {instruments,updated_at_ms:now,error:None};
        },
        Err(_)=>cache.error=Some("Exchange catalog unavailable; retry later".into()),
    }
    cache.clone()
}
fn parse(v:&serde_json::Value)->Option<Instrument> {
    if v["status"]!="Trading" || v["contractType"]!="LinearPerpetual" {return None;}
    Some(Instrument {symbol:v["symbol"].as_str()?.into(),base:v["baseCoin"].as_str()?.into(),quote:v["quoteCoin"].as_str()?.into(),tick_size:v["priceFilter"]["tickSize"].as_str()?.into(),min_qty:v["lotSizeFilter"]["minOrderQty"].as_str()?.into(),qty_step:v["lotSizeFilter"]["qtyStep"].as_str()?.into()})
}
async fn fetch(testnet:bool)->anyhow::Result<Vec<Instrument>> {
    let client=reqwest::Client::builder().timeout(Duration::from_secs(12)).build()?;
    let hosts=if testnet {vec!["https://api-testnet.bybit.com"]}else{vec!["https://api.bytick.com","https://api.bybit.com"]};
    for host in hosts {
        let mut all=Vec::new(); let mut cursor=String::new(); let mut complete=false;
        for _ in 0..30 {
            let response=client.get(format!("{host}/v5/market/instruments-info")).query(&[("category","linear"),("status","Trading"),("limit","1000"),("cursor",cursor.as_str())]).send().await;
            let Ok(response)=response else {break;};
            let Ok(v)=response.json::<serde_json::Value>().await else {break;};
            if v["retCode"]!=0 {break;}
            let Some(rows)=v["result"]["list"].as_array() else {break;};
            all.extend(rows.iter().filter_map(parse));
            let next=v["result"]["nextPageCursor"].as_str().unwrap_or("");
            if next.is_empty() {complete=true;break;}
            if next==cursor {break;}
            cursor=next.into();
        }
        if complete && !all.is_empty() {all.sort_by(|a,b|a.symbol.cmp(&b.symbol));all.dedup_by(|a,b|a.symbol==b.symbol);return Ok(all);}
    }
    anyhow::bail!("catalog unavailable")
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn only_trading_perpetuals() {
        let mut v=serde_json::json!({"symbol":"1000PEPEUSDT","baseCoin":"1000PEPE","quoteCoin":"USDT","status":"Trading","contractType":"LinearPerpetual","priceFilter":{"tickSize":"0.000001"},"lotSizeFilter":{"minOrderQty":"100","qtyStep":"100"}});
        assert_eq!(parse(&v).unwrap().base,"1000PEPE");
        v["status"]="PreLaunch".into();assert!(parse(&v).is_none());
        v["status"]="Trading".into();v["contractType"]="LinearFutures".into();assert!(parse(&v).is_none());
    }
}
