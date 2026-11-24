import { useEffect, useRef, useState } from "react";
import { IChevronDown, IChevronUp, IClose, IGrid, IRefresh, Logo } from "../icons";
import { useHost } from "./HostContext";
import { pluginError, type InstalledPlugin } from "./client";
import type { Category, MediaItem, Page, Section } from "./protocol";
import { usePluginRequests } from "./usePluginRequests";
import PluginMediaDetail from "./PluginMediaDetail";
import { Notice, type OpenMedia } from "./BrowsePages";
import { useStore } from "../store";
import { useAssetUrls } from './MediaCards';
import { readMetadata } from './metadataCache';

type QueueItem = { plugin: InstalledPlugin; item: MediaItem };
type SourcePage = { method: string; args: Record<string, unknown>; cursor?: string; done: boolean; seen: Set<string> };
const shuffle = <T,>(items: T[]) => {
  const result = [...items];
  for (let i = result.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [result[i], result[j]] = [result[j], result[i]]; }
  return result;
};

export const playbackSources = (sources: InstalledPlugin[]) => sources.filter(p => p.methods.includes("media.resolvePlayback") && ["feed.list", "home.getSections", "catalog.list"].some(m => p.methods.includes(m)));

export default function ImmersiveHome({ open, manage, settings, active = true }: { open: OpenMedia; manage: () => void; settings: () => void; active?: boolean }) {
  const host = useHost(), request = usePluginRequests(), app = useStore();
  const available = playbackSources(host.sources);
  const chosen = host.prefs.homeMode === "all" ? available : host.prefs.homeIds.flatMap(id => available.find(p => p.meta.id === id) || []).slice(0, host.prefs.homeMode === "single" ? 1 : undefined);
  const signature = chosen.map(p => p.meta.id + p.revision).join("|");
  const [queue, setQueue] = useState<QueueItem[]>([]), [index, setIndex] = useState(0), [busy, setBusy] = useState(false);
  const [errors, setErrors] = useState<Record<string, string>>({}), [round, setRound] = useState(0);
  const pages = useRef(new Map<string, SourcePage>()), generation = useRef(0), loading = useRef(false), ids = useRef(new Set<string>());
  const [more, setMore] = useState(false);
  const load = async (advance = false) => {
    if (loading.current) return;
    loading.current = true; setBusy(true);
    const epoch = generation.current;
    let appended = false;
    await Promise.all(shuffle(chosen).map(async plugin => {
      const id = plugin.meta.id;
      let state = pages.current.get(id);
      if (state?.done) return [];
      try {
        if (!state) {
          const method = plugin.methods.includes("feed.list") ? "feed.list" : plugin.methods.includes("home.getSections") ? "home.getSections" : "catalog.list";
          const args: Record<string, unknown> = {};
          if (method === "catalog.list") {
            const categories = await request<Category[]>("call", { id, method: "catalog.getCategories", args: {} });
            const category = shuffle(categories)[0];
            args.categoryId = category?.children?.[0]?.id || category?.id;
            if (!args.categoryId) { if (epoch === generation.current) pages.current.set(id, { method, args, done: true, seen: new Set() }); return []; }
          }
          state = { method, args, done: false, seen: new Set() };
        }
        if (epoch !== generation.current) return [];
        const result = await request<Page<MediaItem> | Page<Section>>("call", { id, method: state.method, args: { ...state.args, cursor: state.cursor, limit: 20 } });
        if (epoch !== generation.current) return [];
        if (state.cursor) state.seen.add(state.cursor);
        state.done = !result.next || state.seen.has(result.next);
        state.cursor = result.next || undefined;
        pages.current.set(id, state);
        setErrors(old => { const next = { ...old }; delete next[id]; return next; });
        const items = state.method === "home.getSections" ? (result as Page<Section>).items.flatMap(s => s.items) : (result as Page<MediaItem>).items;
        const added = shuffle(items).filter(item => {
          const key = JSON.stringify([id, item.id]);
          if (ids.current.has(key)) return false;
          ids.current.add(key); return true;
        }).map(item => ({ plugin, item }));
        if (added.length) {
          setQueue(old => [...old, ...added]);
          if (advance && !appended) setIndex(queue.length);
          appended = true;
        }
      } catch (e) {
        if (epoch === generation.current) setErrors(old => ({ ...old, [id]: `${plugin.meta.name}：${pluginError(e)}` }));
        return [];
      }
    }));
    if (epoch !== generation.current) return;
    setMore(chosen.some(p => !pages.current.get(p.meta.id)?.done));
    loading.current = false; setBusy(false);
  };
  useEffect(() => {
    generation.current++; request.cancelAll(); pages.current.clear(); ids.current.clear(); loading.current = false;
    setQueue([]); setIndex(0); setErrors({}); setMore(false); void load();
    return () => { generation.current++; request.cancelAll(); };
  }, [signature, round]);
  const next = () => { if (!active) return; if (index + 1 < queue.length) setIndex(index + 1); else if (more && !busy) void load(true); };
  const previous = () => { if (active) setIndex(Math.max(0, index - 1)); };
  const current = queue[index];
  const before = queue[index - 1], after = queue[index + 1];
  const beforePoster = useAssetUrls(before?.plugin.meta.id || '', before?.item.cover ? [before.item.cover] : [])[0];
  const afterPoster = useAssetUrls(after?.plugin.meta.id || '', after?.item.cover ? [after.item.cover] : [])[0];
  useEffect(() => {
    if (active && more && queue.length - index < 4 && !loading.current) void load();
  }, [active, index, queue.length, more]);
  useEffect(() => {
    if (!active || !after || after.item.entryEpisodeId) return;
    const timer = window.setTimeout(() => { void readMetadata(after.plugin, 'media.getDetail', { id: after.item.id }, false, false).catch(() => {}); }, 600);
    return () => clearTimeout(timer);
  }, [active, after?.plugin.meta.id, after?.plugin.revision, after?.item.id]);
  const loadingHome = !host.ready || busy;
  return <div className="legacy-home">
    {current ? <PluginMediaDetail key={JSON.stringify([current.plugin.meta.id,current.plugin.revision,current.item.id])} active={active} plugin={current.plugin} mediaId={current.item.id} entryEpisodeId={current.item.entryEpisodeId} entryItem={current.item} manage={manage} onSwitch={(pluginId,mediaId,start)=>app.push({kind:"media",pluginId,mediaId,start})} presentation={{mode:"preview",title:current.item.title,queueIndex:index,total:queue.length,onNext:index+1<queue.length||more?next:undefined,onPrevious:index?previous:undefined,nextPreview:after?{title:after.item.title,poster:afterPoster}:undefined,previousPreview:before?{title:before.item.title,poster:beforePoster}:undefined}}/>
      : <div className="absolute inset-0 bg-black flex flex-col items-center justify-center gap-4 text-white/60 text-xs"><Logo size={42}/><p>{loadingHome?"正在加载片源…":host.error|| (available.length?"暂无可播放内容":"添加片源后开始观看")}</p>{!loadingHome&&<button className="rounded-full px-5 py-2 bg-white/10" onClick={available.length?settings:manage}>{available.length?"选择片源":"添加片源"}</button>}</div>}
    {!!Object.keys(errors).length&&<details className="hpp-home-errors"><summary>{Object.keys(errors).length} 个片源暂不可用</summary>{Object.values(errors).map(e=><Notice key={e} error={e}/>)}<button onClick={()=>void load()}>重试片源</button></details>}
  </div>;
}

export function SourcePicker({ plugins, close, manage }: { plugins: InstalledPlugin[]; close: () => void; manage: () => void }) {
  const { prefs, setPrefs } = useHost(), ref = useRef<HTMLDialogElement>(null);
  const [mode, setMode] = useState(prefs.homeMode), [ids, setIds] = useState(prefs.homeIds);
  useEffect(() => { const dialog = ref.current!; dialog.showModal(); return () => dialog.close(); }, []);
  return <dialog ref={ref} className="hpp-source-picker" aria-labelledby="source-picker-title" onCancel={close} onClick={e => { if (e.target === e.currentTarget) close(); }}>
    <div className="hpp-picker-heading"><div><h2 id="source-picker-title">选择首页片源</h2><p>从选中的片源随机播放，选择会自动记住</p></div><button aria-label="关闭片源选择" onClick={close}><IClose size={20}/></button></div>
    <div className="hpp-segmented" role="group" aria-label="片源选择模式">{([["single", "单选"], ["multiple", "多选"], ["all", "全选"]] as const).map(([id, title]) => <button key={id} aria-pressed={mode === id} onClick={() => { setMode(id); if (id === "single") setIds(ids.filter(v => plugins.some(p => p.meta.id === v)).slice(0, 1)); }}>{title}</button>)}</div>
    <div className="hpp-picker-list">{plugins.map(p => <label key={p.meta.id}><span className="hpp-plugin-icon"><IGrid size={20}/></span><span><strong>{p.meta.name}</strong><small>{p.meta.description || "已启用的数据源"}</small></span><input name="home-source" type={mode === "single" ? "radio" : "checkbox"} disabled={mode === "all"} checked={mode === "all" || ids.includes(p.meta.id)} onChange={e => setIds(mode === "single" ? [p.meta.id] : e.target.checked ? [...ids, p.meta.id] : ids.filter(id => id !== p.meta.id))}/></label>)}</div>
    {!plugins.length && <p className="hpp-muted">还没有可播放的片源。</p>}<footer><button onClick={() => { close(); manage(); }}>管理插件</button><button className="primary" disabled={mode !== "all" && !ids.some(id => plugins.some(p => p.meta.id === id))} onClick={() => { setPrefs({ homeMode: mode, homeIds: ids }); close(); }}>保存选择</button></footer>
  </dialog>;
}
