import assert from 'node:assert/strict';
import { build } from 'esbuild';
import { readFileSync } from 'node:fs';

const originalFetch = globalThis.fetch;
let calls = 0;
globalThis.fetch = async () => { calls++; throw new Error('No request expected'); };
const bundled = await build({ entryPoints: ['src/appUpdates.ts'], bundle: true, write: false, format: 'esm', platform: 'node' });
const { compareVersions, checkAppUpdate } = await import(`data:text/javascript;base64,${Buffer.from(bundled.outputFiles[0].text).toString('base64')}`);
assert.equal(calls, 0, 'Importing update support must not check automatically');
for (const [left, right, expected] of [
  ['v0.0.2', '0.0.1', 1], ['v0.10.0', '0.9.0', 1], ['1.0.0', '0.99.99', 1],
  ['v0.0.1', '0.0.1', 0], ['0.0.1', '0.0.2', -1], ['1.0.0+build.2', '1.0.0+build.1', 0],
  ['1.0.0', '1.0.0-rc.1', 1], ['1.0.0-rc.2', '1.0.0-rc.10', -1],
  ['1.0.0-alpha.1', '1.0.0-alpha.beta', -1], ['1.0.0-beta', '1.0.0-alpha', 1],
  ['1.0.0-alpha', '1.0.0-alpha.1', -1], ['1.0.0-rc.1', '1.0.0-rc.1', 0],
]) assert.equal(compareVersions(left, right), expected, `${left} vs ${right}`);
for (const value of ['latest', 'v1.2', '1.02.3', '1.2.3-01', '1.2.3\n', '1.2.3/foo']) {
  assert.throws(() => compareVersions(value, '0.0.1'), /版本号/);
}
const controller = new AbortController();
const release = (tag = 'v0.0.2') => ({ tag_name: tag, html_url: `https://github.com/Minireel/HGhub/releases/tag/${tag}`, draft: false, prerelease: false });
function respond(body, status = 200, headers = {}) {
  globalThis.fetch = async (url, options) => {
    calls++;
    assert.equal(url, 'https://api.github.com/repos/Minireel/HGhub/releases/latest');
    assert.equal(options.credentials, 'omit');
    assert.equal(options.cache, 'no-store');
    assert.equal(options.signal, controller.signal);
    return new Response(JSON.stringify(body), { status, headers });
  };
}
respond(release());
assert.deepEqual(await checkAppUpdate('0.0.1', controller.signal), { tag: 'v0.0.2', url: release().html_url, available: true });
assert.equal((await checkAppUpdate('0.0.2', controller.signal)).available, false);
assert.equal((await checkAppUpdate('0.0.3', controller.signal)).available, false);
respond(release('v1.0.0+build.1'));
assert.equal((await checkAppUpdate('1.0.0', controller.signal)).available, false);
respond({}, 404);
assert.equal(await checkAppUpdate('0.0.1', controller.signal), null);
for (const [body, status, headers, pattern] of [
  [{}, 403, { 'x-ratelimit-remaining': '0' }, /次数已达上限/], [{}, 429, {}, /次数已达上限/],
  [{}, 403, {}, /HTTP 403/], [{}, 500, {}, /HTTP 500/],
  [{ ...release(), prerelease: true }, 200, {}, /发布信息无效/],
  [{ ...release(), draft: true }, 200, {}, /发布信息无效/],
  [{ ...release(), html_url: 'https://example.com/update' }, 200, {}, /发布链接无效/],
  [{ ...release(), html_url: release('v9.0.0').html_url }, 200, {}, /发布链接无效/],
  [{}, 200, {}, /发布信息无效/],
]) { respond(body, status, headers); await assert.rejects(checkAppUpdate('0.0.1', controller.signal), pattern); }
globalThis.fetch = async () => new Response('<html>bad gateway</html>');
await assert.rejects(checkAppUpdate('0.0.1', controller.signal), /发布信息无效/);
globalThis.fetch = async () => { throw new TypeError('Failed to fetch'); };
await assert.rejects(checkAppUpdate('0.0.1', controller.signal), /无法连接 GitHub/);
globalThis.fetch = async (_url, { signal }) => new Promise((_, reject) => signal.addEventListener('abort', () => reject(signal.reason), { once: true }));
const cancelled = checkAppUpdate('0.0.1', controller.signal);
controller.abort();
await assert.rejects(cancelled, { name: 'AbortError' });
globalThis.fetch = originalFetch;

const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
for (const key of ['csp', 'devCsp']) {
  const connect = config.app.security[key].split(';').find(d => d.trim().startsWith('connect-src '));
  assert(connect.split(/\s+/).includes('https://api.github.com'), `${key} permits the release API`);
}
console.log('App update checks passed: version precedence, release validation, errors, cancellation, CSP; no automatic request.');
