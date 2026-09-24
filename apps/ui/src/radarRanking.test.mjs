import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = ts.transpileModule(readFileSync(new URL('./radarRanking.ts', import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { rankedCandidates, revalidatingCandidates } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const now = 1_000_000;
const row = (symbol, quality, extra={}) => ({symbol, quality, universe_rank:0, checked_ms:now, phase:'candidate: confirm live flow', blockers:[], stability_cycles:2, last_qualified_ms:0, peak_quality:quality, ...extra});
test('ranks any qualifying member of 200 by quality, including inclusive boundaries', () => {
  const rows = Array.from({length:200}, (_,i)=>row(`COIN${i}`,0.89,{universe_rank:i+1}));
  rows[199]=row('LAST',1,{universe_rank:200}); rows[120]=row('MID',0.95,{universe_rank:121}); rows[0]=row('FIRST',0.90,{universe_rank:1});
  const ranked=rankedCandidates(rows,now);
  assert.deepEqual(ranked.map(r=>r.symbol),['LAST','MID','FIRST']);
  assert.deepEqual(ranked.map(r=>r.universe_rank),[200,121,1]);
  assert.equal(rows[0].symbol,'FIRST');
});
test('excludes blocked, stale, future, unavailable and malformed scores', () => {
  const rows=[row('BLOCK',1,{blockers:['Spread']}),row('OLD',1,{checked_ms:now-180001}),row('FUTURE',1,{checked_ms:now+1}),row('FAILED',1,{phase:'data unavailable'}),row('UNSTABLE',1,{stability_cycles:1}),row('LOW',0.89999),row('NAN',NaN),row('HIGH',1.01)];
  assert.deepEqual(rankedCandidates(rows,now),[]);
});

test('keeps a recently qualified same-thesis setup visible while it revalidates', () => {
  const rows=[
    row('DEGRADED',0.86,{phase:'forming',blockers:['15m trend does not confirm'],last_qualified_ms:now-60_000,peak_quality:0.97,stability_cycles:4}),
    row('EXPIRED',0.88,{phase:'forming',blockers:['Spread'],last_qualified_ms:now-240_001,peak_quality:0.99,stability_cycles:5}),
    row('ACTIVE',0.94,{last_qualified_ms:now-10_000,peak_quality:0.96,stability_cycles:3}),
  ];
  assert.deepEqual(revalidatingCandidates(rows,now).map(r=>r.symbol),['DEGRADED']);
});
