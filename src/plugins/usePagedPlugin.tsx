import { useEffect, useRef, useState } from "react";
import type { Page } from "./protocol";
import { pluginError, type InstalledPlugin } from "./client";
import { usePluginRequests } from "./usePluginRequests";

/** Append by opaque ID; consumed cursors belong to one query, including under StrictMode. */
export function usePagedPlugin<T extends { id: string }>({ plugin, method, args = {}, limit = 40, enabled = true }: {
  plugin: InstalledPlugin; method: string; args?: Record<string, unknown>; limit?: number; enabled?: boolean;
}) {
  const request = usePluginRequests(), epoch = useRef(0), lock = useRef(false), consumed = useRef(new Set<string>());
  const current = useRef<Page<T> | null>(null), failed = useRef<string | undefined>(undefined);
  const [page, setPage] = useState<Page<T> | null>(null), [busy, setBusy] = useState(false), [error, setError] = useState(""), [warning, setWarning] = useState("");
  const key = JSON.stringify([plugin.meta.id, plugin.revision, method, args, limit, enabled]);
  const load = async (cursor?: string, refresh = false) => {
    if (!enabled || lock.current || cursor !== undefined && consumed.current.has(cursor)) return;
    lock.current = true; const n = epoch.current;
    failed.current = cursor; setBusy(true); setError(""); setWarning("");
    try {
      const result = ['catalog.list', 'home.getSections', 'home.listSection'].includes(method)
        ? await request.read<Page<T>>(plugin, method, { ...args, cursor, limit }, refresh)
        : await request<Page<T>>("call", { id: plugin.meta.id, method, args: { ...args, cursor, limit } });
      if (n !== epoch.current) return;
      if (cursor === undefined) consumed.current.clear(); else consumed.current.add(cursor);
      const oldItems = cursor === undefined ? [] : current.current?.items || [], ids = new Set(oldItems.map(i => i.id));
      const items = [...oldItems, ...result.items.filter(item => { if (ids.has(item.id)) return false; ids.add(item.id); return true; })];
      const repeated = result.next !== undefined && result.next !== null && consumed.current.has(result.next);
      if (repeated) setWarning("来源返回了重复分页游标，已停止继续加载。");
      current.current = { ...result, items, next: repeated ? null : result.next }; setPage(current.current);
    } catch (e) { if (n === epoch.current && (e as { code?: string }).code !== "CANCELLED") setError(pluginError(e)); }
    finally { if (n === epoch.current) { lock.current = false; setBusy(false); } }
  };
  useEffect(() => {
    epoch.current++; lock.current = false; consumed.current.clear(); current.current = null;
    setPage(null); setError(""); setWarning(""); setBusy(false);
    if (enabled) void load();
    return () => { epoch.current++; lock.current = false; request.cancelAll(); };
  }, [key, request]);
  return { page, busy, error, warning, refresh: () => load(undefined, true), retry: () => load(failed.current, true), more: () => load(current.current?.next || undefined) };
}

export function PageLoadMore({ next, busy, error, more, label = "加载更多", auto = true }: {
  next?: string | null; busy: boolean; error: string; more: () => void; label?: string; auto?: boolean;
}) {
  const sentinel = useRef<HTMLDivElement>(null), latest = useRef(more); latest.current = more;
  useEffect(() => {
    const node = sentinel.current;
    if (!auto || !node || !next || busy || error) return;
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) latest.current();
    }, { root: node.closest(".hpp-scroll"), rootMargin: "0px 0px 120px 0px" });
    observer.observe(node); return () => observer.disconnect();
  }, [next, busy, error, auto]);
  return <><div ref={sentinel} className="hpp-page-sentinel" aria-hidden="true"/><button className="hpp-more" disabled={busy || !next} onClick={more}>{busy ? "正在加载…" : next ? label : "没有更多内容"}</button></>;
}
