import assert from 'node:assert/strict';
import { build } from 'esbuild';
const bundle = await build({ entryPoints: ['src/plugins/nextEpisode.ts'], bundle: true, write: false, format: 'esm', platform: 'node' });
const { NextEpisode } = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString('base64')}`);
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
const media = id => ({ sessionId: id, playback: {}, choices: {} });
let now = 1000, calls = 0, warm = [], released = [], cancelled = [];
let pending = deferred();
const next = new NextEpisode({ prepare: () => { calls++; return pending.promise; }, cancel: id => cancelled.push(id),
  release: id => released.push(id), warm: async value => { warm.push(value.sessionId); return true; }, stopWarm() {}, activate: async () => {} }, () => now);
next.offer('episode-2', {}); next.offer('episode-2', {});
const claimed = next.take('episode-2');
pending.resolve(media('one'));
assert.equal((await claimed).sessionId, 'one');
assert.equal(calls, 1); assert.deepEqual(warm, []); // In-flight promotion never starts speculative bytes.
next.clear(); assert.deepEqual(released, []); // Ownership moved to foreground.
pending = deferred(); next.offer('episode-3', {}); next.clear(); pending.resolve(media('late'));
await Promise.resolve(); await Promise.resolve(); assert.ok(released.includes('late'));
pending = deferred(); next.offer('episode-4', {}); pending.resolve(media('expired')); await Promise.resolve();
now += 100_000; assert.equal(await next.take('episode-4'), undefined); assert.ok(released.includes('expired'));
pending = deferred(); next.offer('episode-5', {}); const switched = next.take('episode-5'); next.clear(); pending.resolve(media('cancelled'));
assert.equal(await switched, undefined); assert.ok(released.includes('cancelled'));
assert.ok(cancelled.length >= 2);
const activation = deferred();
const duringActivation = new NextEpisode({ prepare: async () => media('activating'), cancel() {},
  release: id => released.push(id), warm: async () => true, stopWarm() {}, activate: () => activation.promise });
duringActivation.offer('a', {});
const adoption = duringActivation.take('a'); await Promise.resolve(); await Promise.resolve();
duringActivation.clear(); activation.resolve();
assert.equal(await adoption, undefined); assert.ok(released.includes('activating'));

let warmCalls = 0, warmSignal;
const warmGate = deferred();
const interrupted = new NextEpisode({ prepare: async () => media('warm'), cancel() {}, release() {},
  warm: async (_media, params, signal) => { assert.equal(params.episode, 'next'); warmCalls++; warmSignal = signal; await warmGate.promise; return false; },
  stopWarm() {}, activate: async () => {} }, () => now);
interrupted.offer('next', { episode: 'next' }); await Promise.resolve();
interrupted.stopWarm(); assert.equal(warmSignal.aborted, true); warmGate.resolve();
for (let i = 0; i < 4; i++) await Promise.resolve();
for (let i = 0; i < 6; i++) {
  now += 11_000; interrupted.offer('next', { episode: 'next' });
  for (let j = 0; j < 4; j++) await Promise.resolve();
}
assert.equal(warmCalls, 3, 'interrupted warmups have a bounded retry budget');
interrupted.clear(); assert.equal(warmSignal.aborted, true);
console.log('Next episode: reuse, expiry, cancellation during preparation/activation/warmup and bounded retries passed.');
