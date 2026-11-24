const RELEASE_API = "https://api.github.com/repos/Minireel/HGhub/releases/latest";
const RELEASE_PREFIX = "https://github.com/Minireel/HGhub/releases/tag/";

export type AppRelease = { tag: string; url: string; available: boolean };

function versionParts(value: string) {
  const match = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*))?(?:\+[\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*)?$/.exec(value);
  const prerelease = match?.[4]?.split(".") ?? [];
  if (!match || prerelease.some(part => /^0\d+$/.test(part))) throw new Error("无法识别版本号，请稍后重试");
  return { core: match.slice(1, 4).map(BigInt), prerelease };
}

// SemVer precedence: numeric components, then prerelease identifiers; ignore build metadata.
export function compareVersions(left: string, right: string): number {
  const a = versionParts(left), b = versionParts(right);
  for (let i = 0; i < 3; i++) if (a.core[i] !== b.core[i]) return a.core[i] > b.core[i] ? 1 : -1;
  if (!a.prerelease.length || !b.prerelease.length) return Number(!a.prerelease.length) - Number(!b.prerelease.length);
  for (let i = 0; i < Math.max(a.prerelease.length, b.prerelease.length); i++) {
    const x = a.prerelease[i], y = b.prerelease[i];
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    if (x === y) continue;
    const nx = /^\d+$/.test(x), ny = /^\d+$/.test(y);
    if (nx && ny) return BigInt(x) > BigInt(y) ? 1 : -1;
    if (nx !== ny) return nx ? -1 : 1;
    return x > y ? 1 : -1;
  }
  return 0;
}

export async function checkAppUpdate(currentVersion: string, signal: AbortSignal): Promise<AppRelease | null> {
  let response: Response;
  try {
    response = await fetch(RELEASE_API, {
      headers: { Accept: "application/vnd.github+json" },
      credentials: "omit", cache: "no-store", signal,
    });
  } catch (error) {
    if (signal.aborted) throw error;
    throw new Error("无法连接 GitHub，请检查网络后重试");
  }
  if (response.status === 404) return null;
  if (response.status === 429 || response.status === 403 && response.headers.get("x-ratelimit-remaining") === "0") {
    throw new Error("GitHub 请求次数已达上限，请稍后重试");
  }
  if (!response.ok) throw new Error(`检查更新失败（HTTP ${response.status}），请稍后重试`);
  let data;
  try { data = await response.json(); }
  catch { throw new Error("GitHub 返回的发布信息无效，请稍后重试"); }
  if (!data || typeof data.tag_name !== "string" || typeof data.html_url !== "string" || data.draft !== false || data.prerelease !== false) {
    throw new Error("GitHub 返回的发布信息无效，请稍后重试");
  }
  const available = compareVersions(data.tag_name, currentVersion) > 0;
  // Only offer the release page belonging to this repository and this exact tag.
  if (data.html_url !== RELEASE_PREFIX + data.tag_name && data.html_url !== RELEASE_PREFIX + encodeURIComponent(data.tag_name)) {
    throw new Error("GitHub 返回的发布链接无效，请稍后重试");
  }
  return { tag: data.tag_name, url: data.html_url, available };
}
