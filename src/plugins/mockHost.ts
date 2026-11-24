/** In-memory authoring fixture. It does not emulate QuickJS isolation or scheduling. */
import type { Cookie, ErrorCode, HostApi, HostEnv, HttpRequest, HttpResponse, Json, TransferValue } from "./protocol";

export class MockPluginError extends Error {
  constructor(public code: ErrorCode, message = "插件出错", options: { retryAfterSec?: number; retryable?: boolean; details?: Json; cause?: unknown } = {}) {
    super(message); this.name = "PluginError"; Object.assign(this, options);
  }
}
export interface MockHostOptions {
  plugin?: { id: string; version: string }; settings?: Record<string, Json>;
  endpoint?: { id: string; label?: string; url: string } | null;
  now?: () => number; env?: Partial<HostEnv>;
  transport?: (request: HttpRequest) => Promise<HttpResponse<unknown>>;
  webview?: HostApi["webview"];
  crypto?: HostApi["crypto"]; html?: HostApi["html"];
}

export function createMockHost(options: MockHostOptions = {}) {
  const now = options.now || Date.now, data = new Map<string, { value: TransferValue; expires?: number }>();
  const cookies: Cookie[] = [], logs: { level: string; values: unknown[] }[] = [];
  const unsupported = (name: string): never => { throw new MockPluginError("UNSUPPORTED", `假宿主请注入 ${name} 适配器`); };
  const encoder = new TextEncoder(), decoder = new TextDecoder();
  const bytes = (v: string | Uint8Array) => typeof v === "string" ? encoder.encode(v) : v;
  const clone = <T,>(v: T): T => structuredClone(v);
  const active = (key: string) => { const e = data.get(key); if (e?.expires !== undefined && e.expires <= now()) { data.delete(key); return undefined; } return e; };
  const matching = (cookie: Cookie, raw: string) => {
    const u = new URL(raw), domain = cookie.domain || "", path = cookie.path || "/";
    return (u.hostname === domain || !cookie.hostOnly && u.hostname.endsWith(`.${domain}`)) &&
      (u.pathname === path || u.pathname.startsWith(path) && (path.endsWith("/") || u.pathname[path.length] === "/")) &&
      (!cookie.secure || u.protocol === "https:") && (cookie.expiresAt === undefined || cookie.expiresAt > now());
  };
  const features = (options.env?.features || []).filter(feature => feature !== "webview");
  if (options.webview) features.push("webview");
  const hg: HostApi = {
    ...(options.webview ? { webview: options.webview } : {}),
    PluginError: MockPluginError,
    env: { protocol: "1.0.0-dev", app: { name: "HGhub mock", version: "0.0.1" }, plugin: options.plugin || { id: "fixture.mock", version: "1.0.0" }, platform: "windows", locale: "zh-CN", media: { codecs: [], formats: [] }, ...options.env, features },
    endpoint: { current: clone(options.endpoint || null), url(path) { if (!options.endpoint) throw new MockPluginError("UNSUPPORTED", "尚无站点线路"); const u = new URL(path, options.endpoint.url); if (!path.startsWith("/") || path.startsWith("//") || u.origin !== new URL(options.endpoint.url).origin) throw new MockPluginError("INVALID_ARGUMENT", "路径必须位于当前 origin"); return u.href; } },
    settings: { get: <T,>(key: string) => clone(options.settings?.[key]) as T | undefined, getAll: () => clone(options.settings || {}) },
    http: { async fetch<T = string>(request: HttpRequest): Promise<HttpResponse<T>> {
      request.signal?.throwIfAborted();
      if (!options.transport) return unsupported("http.transport");
      const response = await options.transport(request); request.signal?.throwIfAborted(); return clone(response) as HttpResponse<T>;
    } },
    storage: {
      async get<T extends TransferValue>(key: string) { return clone(active(key)?.value) as T | undefined; },
      async set(key, value, settings = {}) { data.set(key, { value: clone(value), expires: settings.ttlSec === undefined ? undefined : now() + settings.ttlSec * 1000 }); },
      async delete(key) { data.delete(key); }, async clear() { data.clear(); },
      async keys(prefix = "") { return [...data.keys()].filter(key => active(key) && key.startsWith(prefix)); },
    },
    cookies: {
      async get({ url }) { return clone(cookies.filter(c => matching(c, url))); },
      async set({ url, cookie }) {
        const u = new URL(url), domain = (cookie.domain || u.hostname).replace(/^\./, "").toLowerCase();
        if (u.hostname !== domain && !u.hostname.endsWith(`.${domain}`)) throw new MockPluginError("INVALID_ARGUMENT", "Cookie domain 不匹配");
        const entry = { ...clone(cookie), domain, path: cookie.path || "/", hostOnly: cookie.hostOnly ?? !cookie.domain };
        const index = cookies.findIndex(c => c.name === entry.name && c.domain === domain && c.path === entry.path); if (index >= 0) cookies.splice(index, 1); cookies.push(entry);
      },
      async clear(params = {}) { for (let i = cookies.length - 1; i >= 0; i--) if (!params.url || matching(cookies[i], params.url)) cookies.splice(i, 1); },
    },
    codec: {
      utf8Encode: s => encoder.encode(s), utf8Decode: b => decoder.decode(b), decodeText: (b, charset) => new TextDecoder(charset).decode(b),
      hexEncode: data => [...bytes(data)].map(b => b.toString(16).padStart(2, "0")).join(""),
      hexDecode(text) { if (!/^(?:[0-9a-f]{2})*$/i.test(text)) throw new MockPluginError("INVALID_ARGUMENT", "无效十六进制"); return Uint8Array.from(text.match(/../g) || [], s => parseInt(s, 16)); },
      base64Encode(data, config = {}) { let binary = ""; for (const b of bytes(data)) binary += String.fromCharCode(b); let result = btoa(binary); if (config.urlSafe) result = result.replace(/\+/g, "-").replace(/\//g, "_"); return config.pad === false ? result.replace(/=+$/, "") : result; },
      base64Decode: text => Uint8Array.from(atob(text.replace(/-/g, "+").replace(/_/g, "/")), c => c.charCodeAt(0)),
    },
    crypto: options.crypto || {
      digest: () => unsupported("crypto.digest"), hmac: () => unsupported("crypto.hmac"), aesEncrypt: () => unsupported("crypto.aesEncrypt"), aesDecrypt: () => unsupported("crypto.aesDecrypt"), rsaEncrypt: () => unsupported("crypto.rsaEncrypt"),
      randomUUID: () => crypto.randomUUID(), randomBytes: length => crypto.getRandomValues(new Uint8Array(length)),
    },
    html: options.html || { parse: () => unsupported("html.parse") },
    log: Object.fromEntries(["debug", "info", "warn", "error"].map(level => [level, (...values: unknown[]) => { logs.push({ level, values: clone(values) }); }])) as HostApi["log"],
  };
  return { hg, logs };
}
