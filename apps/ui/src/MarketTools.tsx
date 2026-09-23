import { useEffect, useMemo, useState } from "react";
export type Instrument = {symbol:string;base:string;quote:string;tick_size:string;min_qty:string;qty_step:string};
export function CoinIcon({base}:{base:string}) {
  const [attempt,setAttempt]=useState(0);
  const coin=base.replace(/^\d+(?=[A-Z])/,'').toLowerCase();
  useEffect(()=>setAttempt(0),[coin]);
  const names:Record<string,string>={btc:'bitcoin',eth:'ethereum',sol:'solana',doge:'dogecoin',shib:'shiba-inu',wif:'dogwifhat',floki:'floki-inu',avax:'avalanche',bnb:'binance-coin',xrp:'xrp'};
  const source=attempt===0?`https://cdn.jsdelivr.net/npm/cryptocurrency-icons@0.18.1/32/color/${coin}.png`:`https://raw.githubusercontent.com/ErikThiart/cryptocurrency-icons/master/32/${names[coin]??coin}.png`;
  return <span className="coin-icon">{attempt<2?<img alt="" loading="lazy" src={source} onError={()=>setAttempt(n=>n+1)}/>:base.replace(/^\d+/,'').slice(0,2)}</span>;
}
export function useCatalog() {
  const [instruments,setInstruments]=useState<Instrument[]>([]);
  const [error,setError]=useState<string|null>(null);
  useEffect(()=>{let alive=true; const controller=new AbortController();
    const load=()=>fetch('/api/instruments',{signal:controller.signal}).then(r=>{if(!r.ok)throw Error('Catalog unavailable');return r.json();}).then(v=>{if(alive){setInstruments(v.instruments??[]);setError(v.error??null);}}).catch(()=>{if(alive)setError('Catalog unavailable');});
    void load();const interval=window.setInterval(load,60_000);
    return()=>{alive=false;controller.abort();clearInterval(interval);};
  },[]);
  return {instruments,error};
}
export function SymbolSearch({instruments,error,busy,onSelect,symbol}:{instruments:Instrument[];error:string|null;busy:boolean;onSelect:(s:string)=>void;symbol:string}) {
  const [query,setQuery]=useState('');const [open,setOpen]=useState(false);const [index,setIndex]=useState(0);
  const results=useMemo(()=>{
    const q=query.toUpperCase().replace(/[^A-Z0-9]/g,'');
    return instruments.filter(i=>!q||i.symbol.startsWith(q)||i.base.includes(q)).sort((a,b)=>{
      const rank=(i:Instrument)=>i.symbol===q||i.base===q?0:i.symbol.startsWith(q)?1:2;
      return rank(a)-rank(b)||a.symbol.localeCompare(b.symbol);
    }).slice(0,60);
  },[query,instruments]);
  function select(s:string){onSelect(s);setOpen(false);setQuery('');setIndex(0);}
  return <div className="symbol-search" onBlur={e=>{if(!e.currentTarget.contains(e.relatedTarget))setOpen(false);}}>
    <span className="search-glyph">⌕</span>
    <input aria-label="Search coins" role="combobox" aria-expanded={open} aria-controls="coin-options" aria-autocomplete="list" aria-activedescendant={open&&results[index]?`coin-${results[index].symbol}`:undefined} placeholder={`Search ${instruments.length || ''} markets · ${symbol}`} value={query} disabled={busy} onFocus={()=>setOpen(true)} onChange={e=>{setQuery(e.target.value);setIndex(0);setOpen(true);}} onKeyDown={e=>{
      if(e.key==='Escape')setOpen(false);
      if(e.key==='ArrowDown'){e.preventDefault();setOpen(true);setIndex(i=>Math.max(0,Math.min(i+1,results.length-1)));}
      if(e.key==='ArrowUp'){e.preventDefault();setIndex(i=>Math.max(0,i-1));}
      if(e.key==='Enter'&&results[index]){e.preventDefault();select(results[index].symbol);}
    }}/>
    {open&&<div className="search-menu"><div className="search-caption">BYBIT PERPETUALS · {instruments.length} markets<br/>Includes listed meme coins. Logos fall back to initials.</div>
      <div id="coin-options" role="listbox">{results.map((i,n)=><button type="button" role="option" aria-selected={n===index} id={`coin-${i.symbol}`} className={n===index?'highlight':''} key={i.symbol} onMouseDown={e=>e.preventDefault()} onClick={()=>select(i.symbol)}><CoinIcon base={i.base}/><span><strong>{i.base}</strong><small>{i.symbol}</small></span><em>{i.quote} PERP</em></button>)}</div>
      {!results.length&&<p>{error??(instruments.length?'No listed perpetual matches this search.':'Loading exchange catalog…')}</p>}
      {error&&instruments.length>0&&<p>{error} · cached list</p>}
    </div>}
  </div>;
}
export function ServiceBar({socketUp,alarms}:{socketUp:boolean;alarms:boolean}) {
  const [services,setServices]=useState<Record<string,string>>({});const [up,setUp]=useState(false);
  useEffect(()=>{let alive=true;const controller=new AbortController();
    async function refresh(){try{const r=await fetch('/api/health',{signal:controller.signal});if(!r.ok)throw Error();const v=await r.json();if(alive){setServices(v.services??{});setUp(true);}}catch{if(alive)setUp(false);}}
    void refresh();const interval=setInterval(refresh,4000);return()=>{alive=false;controller.abort();clearInterval(interval);};
  },[]);
  const items:Record<string,string>={API:up?'online':'offline',Dashboard:socketUp?'connected':'disconnected',...services,Alarms:alarms?'enabled':'off'};
  return <div className="service-bar" aria-label="Service status">{Object.entries(items).map(([k,v])=><span key={k} className={(['online','connected','live','ready','evaluating','enabled','active','configured','paper scoring','paper only'].includes(v) || v.startsWith('deployed'))&&up?'service-good':'service-muted'}><i/>{k.replaceAll('_',' ')} <b>{up||k==='API'?v:'unknown'}</b></span>)}</div>;
}
export function price(value:number) {return Number.isFinite(value)?value.toLocaleString('en-US',{minimumFractionDigits:2,maximumFractionDigits:Math.abs(value)<0.01?10:Math.abs(value)<1?7:Math.abs(value)<100?5:2}):'—';}
