import assert from 'node:assert/strict';
import { build } from 'esbuild';

const bundle = await build({ entryPoints: ['src/plugins/mediaCapabilities.ts', 'src/plugins/client.ts'],
  outdir: 'unused', bundle: true, write: false, format: 'esm', platform: 'node',
  plugins: [{ name: 'capability-fixture', setup(build) {
    build.onResolve({ filter: /^(hls\.js|@tauri-apps\/api\/core)$/ }, args => ({ path: args.path, namespace: 'fixture' }));
    build.onLoad({ filter: /.*/, namespace: 'fixture' }, ({ path }) => ({ contents: path === 'hls.js'
      ? `export default {isSupported:()=>globalThis.fixture.hls, getMediaSource:()=>globalThis.fixture.mse};`
      : `export const invoke=(...args)=>globalThis.fixture.invoke(...args);` }));
  } }],
});
const load = name => import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles.find(f => f.path.endsWith(name)).text).toString('base64')}`);
const { probeMediaCapabilities, hlsPlaybackPath } = await load('mediaCapabilities.js');
const mime = { h264: 'avc1.', hevc: 'hvc1.', av1: 'av01.' };
function setup({ file = [], mse = [], hls = false, native = false, maybe = false, audio = true } = {}) {
  const supports = (type, codecs) => type.startsWith('audio/') ? audio : codecs.some(c => type.includes(mime[c]));
  globalThis.fixture = { hls, mse: { isTypeSupported: type => supports(type, mse) } };
  const video = { canPlayType: type => /mpegurl/i.test(type) ? (native ? 'maybe' : '') : supports(type, file) ? (maybe ? 'maybe' : 'probably') : '' };
  globalThis.document = { createElement: name => { assert.equal(name, 'video'); return video; } };
  return video;
}
setup();
assert.deepEqual(probeMediaCapabilities(), { codecs: [], formats: [] });
setup({ file: ['h264', 'hevc'] });
assert.deepEqual(probeMediaCapabilities(), { codecs: ['h264', 'hevc'], formats: ['progressive'] });
const nativeVideo = setup({ file: ['h264', 'hevc'], native: true });
assert.equal(hlsPlaybackPath(nativeVideo), 'native');
assert.deepEqual(probeMediaCapabilities(), { codecs: ['h264', 'hevc'], formats: ['hls', 'progressive'] });
const mseVideo = setup({ file: ['h264', 'hevc', 'av1'], mse: ['h264', 'av1'], hls: true, native: true });
assert.equal(hlsPlaybackPath(mseVideo), 'mse'); // Same precedence as VideoSurface.
assert.deepEqual(probeMediaCapabilities(), { codecs: ['h264', 'av1'], formats: ['hls', 'progressive'] });
setup({ mse: ['av1'], hls: true });
assert.deepEqual(probeMediaCapabilities(), { codecs: ['av1'], formats: ['hls'] });
setup({ file: ['hevc'], mse: ['h264'], hls: true });
assert.deepEqual(probeMediaCapabilities(), { codecs: [], formats: ['hls', 'progressive'] });
setup({ file: ['h264'], maybe: true });
assert.deepEqual(probeMediaCapabilities(), { codecs: [], formats: [] });
setup({ file: ['h264'], mse: ['h264'], hls: true, audio: false });
assert.deepEqual(probeMediaCapabilities(), { codecs: [], formats: [] });

// First plugin commands must await one registration. Failure must not execute a
// command using a stale/default environment; the next user action can retry.
setup({ file: ['h264'], mse: ['h264'], hls: true });
const { pluginCommand } = await load('client.js');
const calls = [];
let rejectRegistration;
fixture.invoke = async (command, args) => {
  assert.equal(command, 'hpp_manage'); calls.push(args);
  if (args.action === 'host.media.configure') return new Promise((_, reject) => { rejectRegistration = reject; });
  return args.action;
};
const first = pluginCommand('list'), second = pluginCommand('call');
assert.equal(calls.length, 1);
assert.deepEqual(calls[0].params, { codecs: ['h264'], formats: ['hls', 'progressive'] });
rejectRegistration(new Error('fixture IPC failure'));
assert.deepEqual((await Promise.allSettled([first, second])).map(r => r.status), ['rejected', 'rejected']);
assert.equal(calls.length, 1);
fixture.invoke = async (_command, args) => { calls.push(args); return args.action; };
assert.deepEqual(await Promise.all([pluginCommand('list'), pluginCommand('call')]), ['list', 'call']);
assert.deepEqual(calls.map(c => c.action), ['host.media.configure', 'host.media.configure', 'list', 'call']);
console.log('Media capabilities: path probes, conservative codecs, startup barrier and retry passed');
