import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = ts.transpileModule(readFileSync(new URL('./radarRanking.ts', import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { rankedCandidates } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const now = 1_000_000;
const row = (symbol, quality, extra={}) => ({symbol, quality, checked_ms:now, phase:'candidate: confirm live flow', blockers:[], ...extra});
test('ranks any qualifying member of 200 by quality, including inclusive boundaries', () => {
  const rows = Array.from({length:200}, (_,i)=>row(`COIN${i}`,0.89));
  rows[199]=row('LAST',1); rows[120]=row('MID',0.95); rows[0]=row('FIRST',0.90);
  assert.deepEqual(rankedCandidates(rows,now).map(r=>r.symbol),['LAST','MID','FIRST']);
  assert.equal(rows[0].symbol,'FIRST');
});
test('excludes blocked, stale, future, unavailable and malformed scores', () => {
  const rows=[row('BLOCK',1,{blockers:['Spread']}),row('OLD',1,{checked_ms:now-180001}),row('FUTURE',1,{checked_ms:now+1}),row('FAILED',1,{phase:'data unavailable'}),row('LOW',0.89999),row('NAN',NaN),row('HIGH',1.01)];
  assert.deepEqual(rankedCandidates(rows,now),[]);
});
