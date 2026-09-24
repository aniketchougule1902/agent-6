import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = ts.transpileModule(readFileSync(new URL('./tradeGuidance.ts', import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { tradeGuidance } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const now = 1_000_000;
const snapshot = { symbol: 'BTCUSDT', connected: true, feed_stale: false, features: {}, last_price: 100, updated_at_ms: now };
const signal = { symbol: 'BTCUSDT', side: 'long', status: 'active', created_at_ms: now, entry_low: 99, entry_high: 101, stop_loss: 98, tp1: 103, tp2: 105 };
const decide = (s = snapshot, t = signal, connected = true, clock = now, received = now) => tradeGuidance(s, t, connected, clock, received);
test('only fresh in-zone setups are eligible; both directions supported', () => {
  assert.equal(decide().canEnter, true);
  assert.equal(decide(snapshot, {...signal, side: 'short', stop_loss: 102, tp1: 97, tp2: 95}).canEnter, true);
});
test('disconnect, silent socket and frozen engine fail closed', () => {
  assert.equal(decide(snapshot, signal, false).state, 'halt');
  assert.equal(decide(snapshot, signal, true, now + 5001).state, 'halt');
  assert.equal(decide({...snapshot, updated_at_ms: now - 5001}).state, 'halt');
  assert.equal(decide({...snapshot, feed_stale: true}).canEnter, false);
});
test('late, crossed, other-market and terminal setups cannot be entered', () => {
  for (const t of [{...signal, created_at_ms: now - 60001}, {...signal, symbol: 'ETHUSDT'}, ...['tp1_hit','tp2_hit','stop_loss_hit','expired','reversed','invalidated'].map(status => ({...signal,status}))]) {
    assert.equal(decide(snapshot,t).canEnter, false);
  }
  assert.equal(decide({...snapshot,last_price: 102}).canEnter, false);
  assert.equal(decide(snapshot,{...signal,stop_loss: 100}).canEnter, false);
  assert.equal(decide(snapshot,{...signal,entry_low: NaN}).canEnter, false);
  assert.equal(decide(snapshot,null).canEnter, false);
});
