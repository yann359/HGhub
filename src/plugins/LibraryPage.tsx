import { useEffect, useRef, useState } from "react";
import { BrowseFrame, EmptySources, Notice, type OpenMedia } from "./BrowsePages";
import { useHost, libraryKey, type LibraryEntry } from "./HostContext";
import { onPluginAuthFailure, onPluginAuthSuccess, pluginCommand, pluginError, type InstalledPlugin } from "./client";
import { usePluginRequests } from "./usePluginRequests";
import { useAssetUrls } from "./MediaCards";
import type { UserProfile } from "./protocol";
import CloudLibrary from "./CloudLibrary";
import { isMobile } from "../environment";
import { pendingUpdates } from "./updateClient";
import { ICpu, IChevR, ISettings, IStar, IHist, IUser } from "../icons";

export default function LibraryPage({ open, manage, settings }: { open: OpenMedia; manage: (id?: string) => void; settings: () => void }) {
  const host = useHost(), request = usePluginRequests(), [kind, setKind] = useState<"favorite" | "cloud" | "history">("favorite");
  const [trackingOnly, setTrackingOnly] = useState(false);
  const [filter,setFilter]=useState("");
  const [error, setError] = useState(""), [busy, setBusy] = useState(false), [notice, setNotice] = useState("");
  const visible = host.library.filter(e => kind === "history" ? !!e.lastProgress : trackingOnly ? e.tracking : e.favorite)
    .filter(e => host.prefs.mature || !host.plugins.find(p => p.meta.id === e.pluginId)?.meta.mature)
    .filter(e=>e.item.title.toLocaleLowerCase().includes(filter.trim().toLocaleLowerCase()))
    .sort((a, b) => kind === "history" ? (b.lastProgress?.at || 0) - (a.lastProgress?.at || 0) : b.updatedAt - a.updatedAt);
  const checkUpdates = async () => {
    setBusy(true); setError(""); setNotice(""); let count = 0;
    try {
      for (const source of host.sources.filter(p => p.methods.includes("updates.check"))) {
        const entries = host.library.filter(e => e.pluginId === source.meta.id && e.tracking);
        for (let i = 0; i < entries.length; i += 50) {
          const batch = entries.slice(i, i + 50);
          try {
            const results = await request<{ mediaId: string; latestLabel?: string; episodeCount?: number; updatedAt?: number }[]>("call", { id: source.meta.id, method: "updates.check", args: { items: batch.map(e => ({ mediaId: e.mediaId, episodeCount: e.item.episodeCount, updatedAt: e.item.updatedAt })) } });
            for (const result of results) {
              const entry = batch.find(e => e.mediaId === result.mediaId); if (!entry) continue;
              await pluginCommand("library.put", { pluginId: source.meta.id, item: { ...entry.item, episodeCount: result.episodeCount ?? entry.item.episodeCount, updatedAt: result.updatedAt ?? entry.item.updatedAt }, latestLabel: result.latestLabel || "有更新" }); count++;
            }
          } catch (e) { setError(`${source.meta.name}：${pluginError(e)}`); }
        }
      }
      await host.refreshLibrary(); setNotice(`已检查追更，${count} 部作品有变化`);
    } finally { setBusy(false); }
  };
  const accounts = host.sources.filter(p => p.methods.includes("auth.getProfile") || p.methods.includes("library.listMedia"));
  const tabs = <nav className="hpp-library-tabs" aria-label="片库分栏">{([["favorite","收藏",IStar],["cloud","来自片源",IUser],["history","历史",IHist]] as const).map(([id,label,Icon])=><button key={id} aria-pressed={kind===id} onClick={()=>setKind(id)}><Icon size={15}/>{label}{!isMobile&&id!=="cloud"&&" ("+host.library.filter(e=>id==="history"?e.lastProgress:e.favorite).length+")"}</button>)}</nav>;
  const pluginEntry = <button className="library-plugin-entry" onClick={()=>manage()}><span className="library-plugin-icon"><ICpu size={20}/></span><span><strong>{isMobile?"插件中心":"管理片源插件"}</strong>{isMobile&&<small>{host.sources.length} 个来源已启用 · 导入、账号与线路</small>}</span>{pendingUpdates(host.updates).length>0&&<span className="library-update-badge">{pendingUpdates(host.updates).length} 个更新</span>}<IChevR size={16}/></button>;
  return <div className={"hpp-page hpp-library-page refined-page"+(isMobile?" refined-mobile":"")}><header className="hpp-header"><div><h1>{isMobile?"我的":"我的片库"}</h1>{!isMobile&&<p className="library-subtitle">本地收藏、跨片源续播进度与追更动态仅保存在本机</p>}</div>{!isMobile&&pluginEntry}<button aria-label="设置" onClick={settings} className="library-settings"><ISettings size={21}/></button></header>{isMobile&&tabs}<div className="hpp-scroll"><div className="hpp-content">
    {isMobile&&pluginEntry}<div className="library-control-bar">{!isMobile&&tabs}
<input className="library-filter" aria-label="筛选片库作品" placeholder="筛选片库作品…" value={filter} onChange={e=>setFilter(e.target.value)} hidden={kind==="cloud"}/><span className="library-count" hidden={kind==="cloud"}>{visible.length} {kind==="history"?"条记录":"部作品"}{isMobile?" · 仅保存在本机":""}</span>      <div className="hpp-library-toolbar" hidden={kind==="cloud"}>{kind==="favorite"?<><button aria-pressed={trackingOnly} onClick={()=>setTrackingOnly(v=>!v)}>只看追更</button>{(!isMobile||trackingOnly)&&<button disabled={busy} onClick={()=>void checkUpdates()}>{busy?"检查中…":"检查追更"}</button>}</>:<button disabled={!visible.length} onClick={()=>{if(window.confirm("清空本机观看历史？收藏不会被删除。"))void pluginCommand("library.history.clear").then(host.refreshLibrary).catch(e=>setError(pluginError(e)));}}>清空观看历史</button>}</div></div>    <Notice error={error || host.error}/>{notice && <p role="status">{notice}</p>}
    {kind === "cloud" ? <><p className="hpp-library-caption">登录各片源，查看它们的收藏与观看记录。</p>{accounts.map(p=><Account key={p.meta.id+p.revision} plugin={p} open={open} manage={()=>manage(p.meta.id)}/>)}{!accounts.length&&<div className="hpp-empty"><IUser size={36}/><h2>还没有可用的片源账号</h2><p>添加支持账号或云端片库的数据源后，即可在这里查看。</p><button onClick={()=>manage()}>前往插件中心</button></div>}</> : <>

      {kind==="favorite"&&!trackingOnly&&visible.some(e=>e.tracking)&&<section className="library-tracking"><h2>正在追更 · 最新动态</h2><div>{visible.filter(e=>e.tracking).map(entry=><LibraryCard key={libraryKey(entry.pluginId,entry.mediaId)} entry={entry} open={open} history={true} fail={setError} compact/>)}</div></section>}
      <h2 className="library-grid-title">{kind==="history"?"观看历史":trackingOnly?"追更中的作品":"全部收藏"}</h2><section className={"hpp-local-library"+(kind==="history"?" library-history":"")}>{visible.map(entry=><LibraryCard key={libraryKey(entry.pluginId,entry.mediaId)} entry={entry} open={open} history={kind==="history"} fail={setError}/>)}</section>
      {!visible.length&&<div className="hpp-empty">{kind==="history"?<IHist size={38}/>:<IStar size={38}/>}<h2>{kind==="history"?"你的下一段故事，从这里继续":trackingOnly?"还没有追更作品":"把喜欢的作品，留在这里"}</h2><p>{kind==="history"?"播放过的作品与观看进度会自动保存。":"在播放器或作品详情中收藏，无需登录。"}</p></div>}
    </>}
  </div></div></div>;
}
function LibraryCard({entry,open,history,fail,compact=false}:{compact?:boolean;entry:LibraryEntry;open:OpenMedia;history:boolean;fail:(error:string)=>void}) {
  const host=useHost(),plugin=host.sources.find(p=>p.meta.id===entry.pluginId),installed=host.plugins.find(p=>p.meta.id===entry.pluginId);
  const urls=useAssetUrls(entry.pluginId,plugin&&entry.item.cover?[entry.item.cover]:[]),progress=entry.lastProgress;
  return <article className={"hpp-local-entry"+(compact?" library-tracking-card":"")}><button className="hpp-library-cover" aria-label={history ? "继续观看" : "查看作品"} disabled={!plugin?.methods.includes("media.getDetail")} onClick={()=>open(entry.pluginId,entry.mediaId)}><div className="hpp-poster">{urls[0]?<img src={urls[0]} alt="" loading="lazy"/>:<span>{entry.item.title.slice(0,2)}</span>}{!compact&&<small>{!plugin?"来源不可用":entry.latestLabel|| (history?"继续观看":entry.tracking?"追更中":"已收藏")}</small>}{progress?.durationSec&&<progress max={progress.durationSec} value={progress.positionSec}/>}</div><h3>{entry.item.title}</h3></button><p className="hpp-muted">{installed?.meta.name||"来源已移除"}{history&&progress?` · 看到 ${Math.floor(progress.positionSec/60)}:${String(Math.floor(progress.positionSec%60)).padStart(2,"0")}`:""}</p>{compact&&<button className="primary library-continue" disabled={!plugin?.methods.includes("media.getDetail")} onClick={()=>open(entry.pluginId,entry.mediaId)}>{progress?"继续观看":"查看作品"}</button>}<details><summary aria-label={"管理作品 · "+entry.item.title}>•••</summary>{entry.favorite&&<button onClick={()=>void host.put(entry.pluginId,entry.item,{favorite:false}).catch(e=>fail(pluginError(e)))}>取消本地收藏</button>}<AssociatedSources entry={entry} open={open}/></details></article>;
}
function AssociatedSources({ entry, open }: { entry: LibraryEntry; open: OpenMedia }) {
  const host = useHost(), request = usePluginRequests(), [expanded, setExpanded] = useState(false);
  const [sources, setSources] = useState<LibraryEntry[]>([]), [error, setError] = useState("");
  useEffect(() => {
    if (!expanded) return;
    let live = true;
    void request<LibraryEntry[]>("library.sources.get", { pluginId: entry.pluginId, mediaId: entry.mediaId }).then(result => { if (live) setSources(result); }).catch(e => { if (live) setError(pluginError(e)); });
    return () => { live = false; request.cancelAll(); };
  }, [expanded, entry.pluginId, entry.mediaId, request]);
  return <div><button aria-expanded={expanded} onClick={() => setExpanded(v => !v)}>关联来源</button>{expanded && <><Notice error={error}/>{sources.filter(e => host.prefs.mature || !host.plugins.find(p => p.meta.id === e.pluginId)?.meta.mature).map(source => {
    const plugin = host.sources.find(p => p.meta.id === source.pluginId);
    return <p key={libraryKey(source.pluginId, source.mediaId)}><button disabled={!plugin?.methods.includes("media.getDetail")} onClick={() => open(source.pluginId, source.mediaId)}>{source.item.title} · {plugin?.meta.name || source.pluginId}{plugin ? "" : " · 来源不可用"}</button></p>;
  })}{!sources.length && !error && <p className="hpp-muted">尚未关联其他来源</p>}</>}</div>;
}
function Account({ plugin: p, open, manage }: { plugin: InstalledPlugin; open: OpenMedia; manage: () => void }) {
  const request = usePluginRequests(), epoch = useRef(0);
  const [profile, setProfile] = useState<UserProfile | null>(null), [error, setError] = useState("");
  const avatar = useAssetUrls(p.meta.id, profile?.avatar ? [profile.avatar] : []);
  const getProfile = () => {
    const n = ++epoch.current;
    if (p.methods.includes("auth.getProfile")) void request<UserProfile | null>("call", { id: p.meta.id, method: "auth.getProfile", args: {} }).then(r => { if (n === epoch.current) setProfile(r); }).catch(e => { if (n === epoch.current) setError(pluginError(e)); });
  };
  useEffect(() => { getProfile(); return () => { epoch.current++; }; }, [p.meta.id]);
  useEffect(() => onPluginAuthFailure(f => { if (f.id === p.meta.id) { epoch.current++; setProfile(null); if (!f.silent) setError(f.message); } }), [p.meta.id]);
  useEffect(() => onPluginAuthSuccess(id => { if (id === p.meta.id) { setError(""); getProfile(); } }), [p.meta.id]);
  return <section className="hpp-account"><div className="hpp-section-title"><h3>{p.meta.name}</h3><button onClick={manage}>账号与操作</button></div><Notice error={error}/>{profile ? <div className="hpp-profile">{avatar[0] && <img src={avatar[0]} alt=""/>}<strong>{profile.name}</strong><p>{profile.description}</p></div> : p.methods.includes("auth.getProfile") && <p className="hpp-muted">未登录此插件</p>}
    {p.methods.includes("library.listMedia") && (!p.methods.includes("auth.getProfile") || profile) && <CloudLibrary plugin={p} open={open}/>}
  </section>;
}
