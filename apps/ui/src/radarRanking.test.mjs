import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = ts.transpileModule(readFileSync(new URL('./radarRanking.ts', import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { rankedCandidates } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const now = 1_000_000;
const row = (symbol, quality, extra={}) => ({symbol, quality, universe_rank:0, checked_ms:now, phase:'candidate: confirm live flow', blockers:[], ...extra});
test('ranks any qualifying member of 200 by quality, including inclusive boundaries', () => {
  const rows = Array.from({length:200}, (_,i)=>row(`COIN${i}`,0.89,{universe_rank:i+1}));
  rows[199]=row('LAST',1,{universe_rank:200}); rows[120]=row('MID',0.95,{universe_rank:121}); rows[0]=row('FIRST',0.90,{universe_rank:1});
  const ranked=rankedCandidates(rows,now);
  assert.deepEqual(ranked.map(r=>r.symbol),['LAST','MID','FIRST']);
  assert.deepEqual(ranked.map(r=>r.universe_rank),[200,121,1]);
  assert.equal(rows[0].symbol,'FIRST');
});
test('excludes blocked, stale, future, unavailable and malformed scores', () => {
  const rows=[row('BLOCK',1,{blockers:['Spread']}),row('OLD',1,{checked_ms:now-180001}),row('FUTURE',1,{checked_ms:now+1}),row('FAILED',1,{phase:'data unavailable'}),row('LOW',0.89999),row('NAN',NaN),row('HIGH',1.01)];
  assert.deepEqual(rankedCandidates(rows,now),[]);
});
