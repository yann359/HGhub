import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { chromium } from "playwright-core";
import { createServer } from "vite";

// Uses real upstreams, strict HPP networking and Edge decoding. Tauri IPC is replaced by stdio.
const output = process.argv[2];
const data = await mkdtemp(join(tmpdir(), "hghub-bundled-"));
const executable = resolve("src-tauri/crates/hpp/target/debug/examples", process.platform === "win32" ? "dev_host.exe" : "dev_host");
const manifestPath = resolve("bundled-plugins/manifest.json");
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
const manifestUrl = "https://raw.githubusercontent.com/yann359/HGhub/codex/ios-liquid-glass-plugin/bundled-plugins/manifest.json";
const blender = "dev.hghub.blender-open-movies", nasa = "dev.hghub.nasa-svs", resolver = "dev.hghub.nasa-svs-resolver";
const checked = [];
let child, vite, browser;

async function start() {
  const process = child = spawn(executable, [data, "--bundled"], { stdio: ["pipe", "pipe", "inherit"], windowsHide: true });
  const pending = new Map();
  const lines = createInterface({ input: process.stdout });
  const [line] = await once(lines, "line");
  assert.equal(JSON.parse(line).ready, true);
  lines.on("line", line => {
    const result = JSON.parse(line);
    const resolve = pending.get(result.id);
    pending.delete(result.id);
    resolve?.(result);
  });
  return (action, params = {}) => new Promise((resolve, reject) => {
    const id = randomUUID();
    const timer = setTimeout(() => { pending.delete(id); reject(new Error("HPP IPC deadline: " + action)); }, 35000);
    pending.set(id, result => { clearTimeout(timer); resolve(result); });
    process.stdin.write(JSON.stringify({ id, action, params }) + "\n");
  });
}

async function stop() {
  if (!child) return;
  if (child.exitCode === null) {
    const exited = once(child, "exit");
    child.kill();
    await exited;
  }
  child = null;
}

const ok = async (dispatch, action, params = {}) => {
  const result = await dispatch(action, { requestId: randomUUID(), ...params });
  assert.equal(result.ok, true, action + " " + JSON.stringify(result.error || {}));
  return result.value;
};
const call = (dispatch, id, method, args = {}) => ok(dispatch, "call", { id, method, args });
const prepare = (dispatch, id, mediaId, episodeId, options = {}) => ok(dispatch, "media.prepare", {
  id, args: { mediaId, episodeId, reason: "play", serverId: options.serverId }, resolverStreamIndex: options.resolverStreamIndex,
});
const note = label => { checked.push(label); console.log("PASS", label); };

try {
  let dispatch = await start();
  const plugins = await ok(dispatch, "list");
  assert.deepEqual(plugins.map(p => [p.meta.id, p.meta.version, p.status]).sort(), manifest.plugins.map(p => [p.id, p.version, "ready"]).sort());
  const preview = await ok(dispatch, "distribution.preview", { locations: [manifestPath] });
  assert.deepEqual(preview.items.map(item => [item.id, item.version, !!item.error]), manifest.plugins.map(p => [p.id, p.version, false]));
  await ok(dispatch, "distribution.discard", { token: preview.token });
  note("First launch compiled defaults, identity/version and local manifest");
  const remote = await ok(dispatch, "distribution.preview", { locations: [manifestUrl] });
  assert.equal(remote.items.length, 3);
  assert.equal(remote.subscriptions[0].location, manifestUrl);
  await ok(dispatch, "subscriptions.save", { token: remote.token, location: manifestUrl });
  await ok(dispatch, "distribution.discard", { token: remote.token });
  const subscriptionList = await ok(dispatch, "updates.state");
  assert(subscriptionList.subscriptions.some(item => item.location === manifestUrl));
  const checkedUpdates = await ok(dispatch, "updates.check", { ids: manifest.plugins.map(plugin => plugin.id) });
  assert.equal(checkedUpdates.statuses.length, 3);
  assert(checkedUpdates.statuses.every(plugin => plugin.status === "current"), JSON.stringify(checkedUpdates.statuses));
  await ok(dispatch, "distribution.discard", { token: checkedUpdates.preview.token });
  note("Published HTTPS manifest, saved subscription and real version checks");

  for (const id of [blender, nasa]) {
    assert((await call(dispatch, id, "home.getSections")).items[0].items.length > 0);
    const categories = await call(dispatch, id, "catalog.getCategories");
    assert((await call(dispatch, id, "catalog.list", { categoryId: categories[0].id })).items.length > 0);
  }
  assert.equal((await call(dispatch, blender, "search.search", { query: "Sintel" })).items[0].id, "sintel");
  assert.equal((await call(dispatch, nasa, "search.search", { query: "月相" })).items[0].id, "5187");
  const moon = await call(dispatch, nasa, "media.getDetail", { id: "5187" });
  assert.equal(moon.cover.url.startsWith("https://svs.gsfc.nasa.gov/"), true);
  assert.deepEqual(moon.playlists[0].episodes.map(e => e.id), ["376413", "376417", "376419"]);
  note("Real catalogue, category, search, detail and genuine NASA episodes");

  const range = async url => {
    const response = await fetch(url, { headers: { Range: "bytes=0-511", Origin: "tauri://localhost" }, signal: AbortSignal.timeout(20000) });
    assert.equal(response.status, 206, "expected proxy Range response");
    assert.equal(response.headers.get("Access-Control-Allow-Origin"), "*");
    assert.equal(Buffer.from(await response.arrayBuffer()).subarray(4, 8).toString(), "ftyp");
  };
  const mp4 = await prepare(dispatch, nasa, "5187", "376413", { resolverStreamIndex: 1 });
  assert.equal(mp4.providerId, resolver);
  assert.equal(mp4.choices.resolver.streams[1].height, 720);
  await range(mp4.url);
  await range((await prepare(dispatch, nasa, "5187", "376417")).url);
  await range((await prepare(dispatch, blender, "big-buck-bunny", "big-buck-bunny", { serverId: "archive" })).url);
  const hls = await prepare(dispatch, blender, "big-buck-bunny", "big-buck-bunny", { serverId: "mux" });
  assert.equal(hls.format, "hls");
  const playlist = await fetch(hls.url, { signal: AbortSignal.timeout(20000) });
  assert.equal(playlist.status, 200);
  assert.match(await playlist.text(), /^#EXTM3U/);
  const sintel = await prepare(dispatch, blender, "sintel", "sintel");
  const subtitle = await fetch(sintel.playback.subtitles[0].url, { signal: AbortSignal.timeout(20000) });
  assert.equal(subtitle.status, 200);
  assert.match(await subtitle.text(), /^WEBVTT/);
  note("Real NASA API resolver, 720p selection, episode change, Blender MP4/HLS lines and SRT-to-VTT");

  vite = await createServer({ server: { port: 0, strictPort: false, host: "127.0.0.1" } });
  await vite.listen();
  const url = "http://127.0.0.1:" + vite.httpServer.address().port;
  browser = await chromium.launch({ headless: true, executablePath: process.env.BROWSER_EXECUTABLE || (process.platform === "win32" ? "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" : undefined), args: ["--autoplay-policy=no-user-gesture-required"] });
  const context = await browser.newContext({ viewport: { width: 440, height: 956 }, isMobile: true, hasTouch: true, userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 27_0 like Mac OS X) AppleWebKit/605.1.15 Version/27.0 Mobile/15E148 Safari/604.1" });
  const page = await context.newPage();
  await page.route("**/__hpp_test_ipc", async route => {
    const { action, params } = JSON.parse(route.request().postData());
    const body = await dispatch(action, params);
    await route.fulfill({ json: body });
  });
  const loadedVideo = () => page.waitForFunction(() => {
    const video = document.querySelector(".hg-watching video");
    return video?.videoWidth > 0 && video.readyState >= 3 && !video.paused && video.currentTime > .4;
  }, null, { timeout: 35000 });

  await page.goto(url + "/tests/plugins.html");
  const tabs = page.getByRole("navigation", { name: "主导航" });
  for (const tab of await tabs.getByRole("button").all()) {
    const box = await tab.boundingBox();
    assert(box && box.width >= 44 && box.height >= 44);
  }
  await tabs.getByRole("button", { name: "我的", exact: true }).click();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const appearance = page.getByRole("combobox", { name: "外观模式", exact: true });
  await appearance.selectOption("system");
  await page.emulateMedia({ colorScheme: "light" });
  await page.waitForFunction(() => document.documentElement.classList.contains("theme-light"));
  await page.emulateMedia({ colorScheme: "dark" });
  await page.waitForFunction(() => !document.documentElement.classList.contains("theme-light"));
  await appearance.selectOption("light");
  await page.reload();
  await page.waitForFunction(() => document.documentElement.classList.contains("theme-light"));
  note("iPhone-size glass navigation and persisted light, dark and system themes");

  for (const { format, title, query } of [
    { format: "HLS", title: /Big Buck Bunny/, query: "大雄兔" },
    { format: "MP4", title: /月相与天平动/, query: "月相" },
  ]) {
    await page.goto(url + "/tests/plugins.html");
    const navigation = page.getByRole("navigation", { name: "主导航" });
    await page.locator(".hg-preview-copy h1").waitFor({ timeout: 30000 });
    await navigation.getByRole("button", { name: "搜索", exact: true }).click();
    const search = page.getByRole("textbox", { name: "搜索所有插件" });
    await search.fill(query);
    await search.press("Enter");
    await page.locator(".legacy-search-poster").first().click({ timeout: 15000 });
    const dialog = page.getByRole("dialog", { name: "作品详情" });
    await dialog.getByRole("heading", { name: title }).waitFor({ timeout: 15000 });
    await dialog.locator(".hg-watch-action").click({ timeout: 15000 });
    await loadedVideo();
    note("React iPhone-size home, search, detail and Edge decoded real " + format + " through Rust proxy");
    if (output) await page.screenshot({ path: join(resolve(output, ".."), "ios-full-" + format.toLowerCase() + "-edge.png") });
    if (format === "HLS") {
      await page.getByRole("button", { name: "更多", exact: true }).click();
      await page.getByRole("button", { name: "画质与线路", exact: true }).click();
      await page.getByLabel("源播放线路", { exact: true }).selectOption("archive");
      await page.waitForFunction(() => document.querySelector(".hg-watching video")?.src.startsWith("http://127.0.0.1:"));
      await loadedVideo();
      note("Switching from Mux HLS to the authorized Internet Archive MP4 played in Edge");
    } else {
      await page.getByRole("button", { name: "选集", exact: true }).click();
      await page.getByRole("button", { name: "月球轨道", exact: true }).click();
      await page.waitForFunction(() => document.querySelector(".hg-watch-title p")?.textContent.includes("月球轨道"));
      await loadedVideo();
      note("NASA episode switching played the second upstream asset in Edge");
    }
  }
  await browser.close(); browser = null;
  await vite.close(); vite = null;

  await ok(dispatch, "enable", { id: resolver, enabled: false });
  const bad = await dispatch("media.prepare", { id: nasa, args: { mediaId: "5187", episodeId: "376413", reason: "play" }, requestId: randomUUID() });
  assert.equal(bad.ok, false);
  await range((await prepare(dispatch, blender, "big-buck-bunny", "big-buck-bunny", { serverId: "archive" })).url);
  await ok(dispatch, "remove", { id: nasa });
  await stop();
  dispatch = await start();
  const restarted = await ok(dispatch, "list");
  assert.equal(restarted.find(p => p.meta.id === resolver).status, "disabled");
  assert(!restarted.some(p => p.meta.id === nasa));
  assert.equal(restarted.find(p => p.meta.id === blender).status, "ready");
  note("Resolver fault isolated; disabled and removed defaults stay that way after restart");
} finally {
  await browser?.close();
  await vite?.close();
  await stop();
  await rm(data, { recursive: true, force: true });
  if (output) await writeFile(output, JSON.stringify({ environment: "Windows HPP + Edge, real upstreams, test IPC bridge", checked }, null, 2) + "\n");
}
