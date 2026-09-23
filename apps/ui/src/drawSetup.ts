import type { IChartApi, ISeriesApi, UTCTimestamp } from "lightweight-charts";
import type { Candle } from "./generated/Candle";
import type { TradeSignal } from "./generated/TradeSignal";
import { overlayData } from "./chartIndicators";
import { price } from "./MarketTools";
export function drawSetup(ctx:CanvasRenderingContext2D,chart:IChartApi,series:ISeriesApi<'Candlestick'>,candles:Candle[],data:ReturnType<typeof overlayData>,signal:TradeSignal|null,ribbon:boolean,zones:boolean,width:number,height:number) {
 ctx.clearRect(0,0,width,height);
 const paneHeight=chart.panes()[0]?.getHeight()??height;
 const plotWidth=chart.timeScale().width();
 ctx.save();ctx.beginPath();ctx.rect(0,0,plotWidth,paneHeight);ctx.clip();
 const x=(t:UTCTimestamp)=>chart.timeScale().timeToCoordinate(t);
 const y=(p:number)=>series.priceToCoordinate(p);
 if(ribbon){
   const fast=new Map(data.ema21.map(p=>[p.time,p.value]));
   for(let i=1;i<data.ema50.length;i++){
     const a=data.ema50[i-1],b=data.ema50[i];const fa=fast.get(a.time),fb=fast.get(b.time);
     if(fa===undefined||fb===undefined)continue;
     const xa=x(a.time),xb=x(b.time),ya=y(a.value),yb=y(b.value),yfa=y(fa),yfb=y(fb);
     if(xa===null||xb===null||ya===null||yb===null||yfa===null||yfb===null||xb<0||xa>plotWidth)continue;
     ctx.fillStyle=fb>=b.value?'rgba(38,174,219,0.20)':'rgba(230,61,109,0.20)';
     ctx.beginPath();ctx.moveTo(xa,ya);ctx.lineTo(xb,yb);ctx.lineTo(xb,yfb);ctx.lineTo(xa,yfa);ctx.closePath();ctx.fill();
   }
 }
 if(zones&&signal&&candles.length){
   const last=candles[candles.length-1];const duration=candles.length>1?last.start_ms-candles[candles.length-2].start_ms:60000;
   const start=Math.floor(signal.created_at_ms/duration)*duration;
   const end=signal.last_event_ms>signal.created_at_ms&&['tp2_hit','stop_loss_hit','expired'].includes(signal.status)?signal.last_event_ms:last.start_ms;
   const left=x(Math.floor(start/1000) as UTCTimestamp);
   const right=x(Math.floor(Math.floor(end/duration)*duration/1000) as UTCTimestamp);
   const entry=(signal.entry_low+signal.entry_high)/2;
   const ey=y(entry),sy=y(signal.stop_loss),ty=y(signal.tp2);
   if(left!==null&&ey!==null&&sy!==null&&ty!==null){
     const l=Math.max(0,left),r=Math.min(plotWidth,Math.max(l+120,(right??plotWidth-15)+35));
     ctx.fillStyle='rgba(27,205,148,.16)';ctx.fillRect(l,Math.min(ey,ty),r-l,Math.abs(ty-ey));
     ctx.fillStyle='rgba(246,72,103,.18)';ctx.fillRect(l,Math.min(ey,sy),r-l,Math.abs(sy-ey));
     for(const [py,label,color] of [[ty,`TP2 ${price(signal.tp2)} · ${signal.risk_reward_tp2.toFixed(1)}R`,'#35d399'],[ey,`ENTRY ${price(entry)}`,'#d8e4f7'],[sy,`SL ${price(signal.stop_loss)}`,'#ff5c75']] as const){
       ctx.strokeStyle=color;ctx.setLineDash([5,4]);ctx.beginPath();ctx.moveTo(l,py);ctx.lineTo(r,py);ctx.stroke();ctx.setLineDash([]);
       ctx.font='bold 11px system-ui';const w=ctx.measureText(label).width+16;
       ctx.fillStyle='#111827';ctx.fillRect(Math.max(0,r-w),py-20,w,19);ctx.fillStyle=color;ctx.fillText(label,Math.max(0,r-w)+8,py-7);
     }
   }
 }
 ctx.restore();
}
