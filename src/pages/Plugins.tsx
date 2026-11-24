import { useEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { ICpu, IBack, IPlus } from "../icons";
import { pluginError, type InstalledPlugin } from "../plugins/client";
import type { Endpoint, Setting } from "../plugins/protocol";
import { usePluginRequests } from "../plugins/usePluginRequests";
import PluginForms from "../plugins/PluginForms";
import PluginDistribution from "../plugins/PluginDistribution";
import { useHost } from "../plugins/HostContext";
import { isMobile } from "../environment";
import { registerBackHandler } from "../back";
import { movedBefore } from "../plugins/HomeArrangement";
const statuses: Record<InstalledPlugin["status"], string> = { ready: "已就绪", unloaded: "待加载", failed: "运行失败", incompatible: "不兼容", disabled: "已停用" };
function ErrorMessage({ error }: { error: string }) { return error ? <div className="hpp-error" role="alert">{error}</div> : null; }
export default function Plugins({onBack,focusId}: {onBack:()=>void;focusId?:string}) {
  const host=useHost();
  const plugins=host.plugins.filter(p=>host.prefs.mature||!p.meta.mature);
  return <div className={`hpp-page hpp-plugins-page refined-page${isMobile ? " refined-mobile" : ""}`}><ErrorMessage error={host.error}/><PluginManagement plugins={plugins} reload={host.reload} focusId={focusId} onBack={onBack}/></div>;
}
function PluginManagement({ plugins, reload, focusId = "", onBack }: { plugins: InstalledPlugin[]; reload: () => Promise<void>; focusId?: string; onBack: () => void }) {
  const request = usePluginRequests();
  const { plugins: installedPlugins, prefs, setPrefs } = useHost();
  const drag = useRef<{ id: string; kind: "source" | "resolver"; pointerId: number; startY: number; y: number; timer: number; active: boolean; targetId: string; after: boolean } | null>(null);
  const [dragging, setDragging] = useState<{ id: string; offsetY: number } | null>(null), [dropTarget, setDropTarget] = useState<{ id: string; edge: "before" | "after" } | null>(null);
  const [error, setError] = useState(""), [busy, setBusy] = useState(false);
  const [options, setOptions] = useState<string | null>(focusId || null);
  const [tab, setTab] = useState<"installed" | "import">("installed");
  useEffect(() => { if (focusId) { setOptions(focusId); setTab("installed"); } }, [focusId]);
  const perform = async (action: string, params: Record<string, unknown>) => { setBusy(true); setError(""); try { await request(action, params); await reload(); } catch (e) { setError(pluginError(e)); } finally { setBusy(false); } };
  const selectedPlugin=plugins.find(p=>p.meta.id===options);
  useEffect(() => {
    if (isMobile && selectedPlugin && tab === "installed") return registerBackHandler(() => { setOptions(null); return true; }, 20);
  }, [selectedPlugin, tab]);
  const [filter,setFilter]=useState("");
  const [kindFilter,setKindFilter]=useState("all");
  const orderedPlugins = useMemo(() => [...plugins].sort((a,b) => {
    const ai=prefs.order.indexOf(a.meta.id), bi=prefs.order.indexOf(b.meta.id);
    return (ai<0?Number.MAX_SAFE_INTEGER:ai)-(bi<0?Number.MAX_SAFE_INTEGER:bi);
  }), [plugins,prefs.order]);
  const orderIds = (kind: "source" | "resolver") => installedPlugins
    .filter(p => (p.meta.kind === "resolver" ? "resolver" : "source") === kind)
    .map(p => p.meta.id)
    .sort((a,b) => {
      const ai=prefs.order.indexOf(a), bi=prefs.order.indexOf(b);
      return (ai<0?Number.MAX_SAFE_INTEGER:ai)-(bi<0?Number.MAX_SAFE_INTEGER:bi);
    });
  const commitOrder = (kind: "source" | "resolver", from: string, target: string, after: boolean) => {
    const ids=orderIds(kind);
    let reordered=movedBefore(ids,from,target);
    if(after&&from!==target){reordered=reordered.filter(id=>id!==from);reordered.splice(reordered.indexOf(target)+1,0,from);}
    const sourceIds=kind==="source"?reordered:orderIds("source");
    const resolverIds=kind==="resolver"?reordered:orderIds("resolver");
    const installedIds=new Set(installedPlugins.map(p=>p.meta.id));
    setPrefs({order:[...sourceIds,...resolverIds,...prefs.order.filter(id=>!installedIds.has(id))]});
  };
  const clearDrag = () => {
    const current=drag.current;
    if(current)window.clearTimeout(current.timer);
    drag.current=null;setDragging(null);setDropTarget(null);
  };
  const beginDrag = (e: ReactPointerEvent<HTMLButtonElement>, id: string, kind: "source" | "resolver") => {
    if(e.button!==0||busy)return;
    e.preventDefault();
    const current={id,kind,pointerId:e.pointerId,startY:e.clientY,y:e.clientY,timer:0,active:false,targetId:id,after:false};
    drag.current=current;
    e.currentTarget.setPointerCapture(e.pointerId);
    current.timer=window.setTimeout(()=>{current.active=true;setDragging({id,offsetY:0});},280);
  };
  const moveDrag = (e: ReactPointerEvent<HTMLButtonElement>) => {
    const current=drag.current;
    if(!current||current.pointerId!==e.pointerId)return;
    current.y=e.clientY;
    if(!current.active){
      if(Math.abs(e.clientY-current.startY)>8){window.clearTimeout(current.timer);drag.current=null;}
      return;
    }
    e.preventDefault();
    setDragging({id:current.id,offsetY:e.clientY-current.startY});
    const target=document.elementsFromPoint(e.clientX,e.clientY).map(element=>element.closest<HTMLElement>("[data-plugin-id]")).find(element=>element&&element.dataset.pluginId!==current.id&&element.dataset.pluginKind===current.kind);
    if(!target?.dataset.pluginId){current.targetId=current.id;current.after=false;setDropTarget(null);return;}
    const rect=target.getBoundingClientRect();
    current.targetId=target.dataset.pluginId;
    current.after=e.clientY>rect.top+rect.height/2;
    setDropTarget({id:current.targetId,edge:current.after?"after":"before"});
  };
  const endDrag = (e: ReactPointerEvent<HTMLButtonElement>) => {
    const current=drag.current;
    if(current&&current.pointerId===e.pointerId&&current.active)commitOrder(current.kind,current.id,current.targetId,current.after);
    clearDrag();
  };
  const keyMove = (id: string, kind: "source" | "resolver", direction: -1 | 1) => {
    const ids=orderIds(kind), index=ids.indexOf(id), target=ids[index+direction];
    if(target)commitOrder(kind,id,target,direction<0);
  };
  return <><header className="hpp-header"><button aria-label="返回" onClick={()=>isMobile&&selectedPlugin&&tab==="installed"?setOptions(null):onBack()}><IBack size={20}/></button><div><h1>{isMobile&&selectedPlugin&&tab==="installed"?selectedPlugin.meta.name:"插件中心"}</h1>{!isMobile&&<p>管理内容来源、解析器、站点线路与 HPP 协议声明的插件界面</p>}</div>{isMobile&&<button className="plugin-mobile-import" aria-label="导入插件" onClick={()=>setTab("import")}><IPlus size={22}/></button>}</header><nav className="hpp-provider-tabs" aria-label="插件中心分栏" hidden={isMobile&&!!selectedPlugin&&tab==="installed"}>
    <div className="plugin-management-segments">
      <button aria-pressed={tab === "installed"} onClick={() => setTab("installed")}>{isMobile?`已安装 ${plugins.length}`:`已安装插件 (${plugins.length})`}</button>
      <button aria-pressed={tab === "import"} onClick={() => setTab("import")}>{isMobile?"导入与订阅":"导入与列表订阅"}</button>
    </div>
    <button className="primary plugin-quick-import" onClick={()=>{setTab("import");requestAnimationFrame(()=>document.querySelector<HTMLInputElement>(".hpp-distribution-location input")?.focus());}}><IPlus size={15}/>快速导入</button>
  </nav><div className="hpp-scroll"><div className="hpp-content">
    <div hidden={tab !== "import"} className="plugin-import-layout"><PluginDistribution plugins={plugins} reload={reload} /></div>
    <div hidden={tab !== "installed"}>
    <div className={`refined-plugin-workspace${selectedPlugin?" has-selection":""}`}><div className="refined-plugin-list"><input className="refined-plugin-filter" aria-label="筛选插件名称" placeholder="筛选插件名称…" value={filter} onChange={e=>setFilter(e.target.value)}/>
    <nav className="plugin-kind-tabs" aria-label="插件类型">{[["all","全部"],["source","内容源"],["resolver","解析器"]].map(([id,label])=><button key={id} aria-pressed={kindFilter===id} onClick={()=>setKindFilter(id)}>{label} <small>{plugins.filter(p=>id==="all"||(p.meta.kind==="resolver"?"resolver":"source")===id).length}</small></button>)}</nav>
    <ErrorMessage error={error} />

    {!plugins.length && <div className="hpp-empty"><ICpu size={40}/><h2>还没有安装插件</h2><p>导入数据源开始观看，或添加解析器拓展播放能力。</p><button className="primary" onClick={() => setTab("import")}>导入插件</button></div>}
    {(["source","resolver"] as const).map(kind => <section className="hpp-plugin-group" key={kind}>{plugins.some(p => (p.meta.kind === "resolver") === (kind === "resolver") && (kindFilter==="all"||kindFilter===kind)) && <h2>{kind === "source" ? "数据源" : "解析器"}<small>{plugins.filter(p => (p.meta.kind === "resolver") === (kind === "resolver")).length}</small></h2>}<div className="hpp-installed">{orderedPlugins.filter(p => (p.meta.kind === "resolver") === (kind === "resolver") && (kindFilter==="all"||kindFilter===kind) && (!filter || p.meta.name.toLocaleLowerCase().includes(filter.toLocaleLowerCase()))).map(p => <article data-plugin-id={p.meta.id} data-plugin-kind={kind} className={`hpp-plugin ${options===p.meta.id?"is-selected":""} ${dragging?.id===p.meta.id?"is-dragging":""} ${dropTarget?.id===p.meta.id?`is-drop-${dropTarget.edge}`:""}`} style={dragging?.id===p.meta.id?({"--plugin-drag-y":`${dragging.offsetY}px`} as CSSProperties):undefined} key={p.meta.id}>
      <button className="hpp-plugin-heading plugin-select" aria-label={"查看 "+p.meta.name+" 的设置与日志"} aria-pressed={options===p.meta.id} onClick={()=>setOptions(p.meta.id)}><span className="hpp-plugin-icon refined-glyph">{p.meta.name.slice(0,1)}</span><div><h3>{p.meta.name}</h3><p>v{p.meta.version} · {p.meta.kind==="resolver"?"解析器":"内容源"}</p></div><span className={`hpp-status ${p.status}`}>{statuses[p.status]}</span></button>
      <button className="plugin-enable-switch" role="switch" aria-checked={p.enabled} aria-label={"启用 "+p.meta.name} disabled={busy} onClick={()=>void perform("enable",{id:p.meta.id,enabled:!p.enabled})}><span/></button>
      <button className="plugin-reorder-handle" aria-label={`长按拖动 ${p.meta.name} 排序`} title="长按拖动排序；键盘可用上下方向键" onPointerDown={e=>beginDrag(e,p.meta.id,kind)} onPointerMove={moveDrag} onPointerUp={endDrag} onPointerCancel={clearDrag} onLostPointerCapture={clearDrag} onKeyDown={e=>{if(e.key==="ArrowUp"||e.key==="ArrowDown"){e.preventDefault();keyMove(p.meta.id,kind,e.key==="ArrowUp"?-1:1);}}}>⠿</button>

    </article>)}</div></section>)}
    </div><aside className="refined-plugin-detail">{selectedPlugin?<><div className="plugin-overview"><span className="refined-glyph">{selectedPlugin.meta.name.slice(0,1)}</span><div><h2>{selectedPlugin.meta.name} <small>v{selectedPlugin.meta.version}</small></h2><p>{selectedPlugin.meta.id} · {statuses[selectedPlugin.status]}</p><p>{selectedPlugin.meta.description}</p><small>{selectedPlugin.origin?.manifestName||selectedPlugin.source||"粘贴代码"}</small></div><button onClick={()=>setOptions(null)}>收起</button></div><ErrorMessage error={selectedPlugin.error?.message||""}/><PluginOptions key={selectedPlugin.meta.id+selectedPlugin.revision} plugin={selectedPlugin} reload={reload}><section className="plugin-maintenance"><h3>版本维护</h3>{selectedPlugin.enabled&&selectedPlugin.status!=="ready"&&<button disabled={busy} onClick={()=>void perform("retry",{id:selectedPlugin.meta.id})}>重试初始化</button>}{selectedPlugin.canRestore&&<button disabled={busy} onClick={()=>void perform("restore",{id:selectedPlugin.meta.id})}>恢复上一可用版本</button>}<button onClick={()=>setTab("import")}>插件更新与订阅</button><button className="danger" disabled={busy} onClick={()=>{if(window.confirm("移除「"+selectedPlugin.meta.name+"」及其本地存储与 Cookie？"))void perform("remove",{id:selectedPlugin.meta.id});}}>移除插件</button></section></PluginOptions></>:<div className="hpp-empty"><ICpu size={32}/><h2>插件详情</h2><p>从左侧选择插件，查看账号、线路、设置与日志。</p></div>}</aside></div>
    </div>
  </div></div></>;
}

function PluginOptions({ plugin: p, reload, children }: { plugin: InstalledPlugin; reload: () => Promise<void>; children?: ReactNode }) {
  const request = usePluginRequests();
  const [settings, setSettings] = useState<Record<string, unknown>>({}), [endpoints, setEndpoints] = useState<Endpoint[]>([]), [logs, setLogs] = useState<unknown[]>([]), [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [probes, setProbes] = useState<{id:string;url:string;ok:boolean;latencyMs:number;status?:number;error?:string}[]>([]);
  const [settingsLoaded, setSettingsLoaded] = useState(false), [notice, setNotice] = useState("");
  useEffect(() => {
    void request<Record<string, unknown>>("settings.get", { id: p.meta.id }).then(values => { setSettings(values); setSettingsLoaded(true); }).catch(e => setError(pluginError(e)));
    if (p.enabled && p.methods.includes("endpoints.list")) void request<Endpoint[]>("endpoint.list", { id: p.meta.id, refresh: false }).then(setEndpoints).catch(e => setError(pluginError(e)));
  }, [p.meta.id]);
  const update = async (action: string, data: Record<string, unknown>) => {
    setBusy(true); setError(""); setNotice(""); try { await request(action, { id: p.meta.id, ...data }); await reload(); setNotice("设置已保存"); } catch (e) { setError(pluginError(e)); } finally { setBusy(false); }
  };
  return <div className="hpp-options"><ErrorMessage error={error} />
    <div className="plugin-forms-column"><PluginForms plugin={p} /></div><div className="plugin-options-column">
    {p.meta.settings?.map(s => <div className="hpp-setting" key={s.key}><label htmlFor={`setting-${s.key}`}>{s.label}<small>{s.description}</small></label><SettingInput setting={s} value={settings[s.key]} disabled={busy || !settingsLoaded} onChange={v => setSettings(old => ({ ...old, [s.key]: v }))} /><button disabled={busy || !settingsLoaded} onClick={() => void update("settings.set", { key: s.key, value: settings[s.key] })}>保存</button></div>)}
    {notice && <p role="status" className="hpp-muted">{notice}</p>}
    {!!endpoints.length && <label className="hpp-setting">站点线路<select value={p.endpoint?.id || ""} disabled={busy} onChange={e => void update("endpoint.set", { endpointId: e.target.value })}>{endpoints.map(e => <option value={e.id} key={e.id}>{e.label || e.id} · {e.url}</option>)}</select></label>}
    {p.enabled && p.methods.includes("endpoints.list") && <div className="hpp-distribution-buttons"><button disabled={busy} onClick={() => {
      setBusy(true); setError(""); void request<Endpoint[]>("endpoint.list", {id:p.meta.id,refresh:true}).then(async r=>{setEndpoints(r);setProbes([]);await reload();}).catch(e=>setError(pluginError(e))).finally(()=>setBusy(false));
    }}>刷新站点线路</button><button disabled={busy} onClick={() => {
      setBusy(true);setError("");void request<typeof probes>("endpoint.probe", {id:p.meta.id}).then(setProbes).catch(e=>setError(pluginError(e))).finally(()=>setBusy(false));
    }}>探测站点线路</button></div>}
    {!!probes.length && <ul className="hpp-probes">{probes.map(result=><li key={result.id}>{endpoints.find(e=>e.id===result.id)?.label||result.id} · {result.ok?`可达 · ${result.latencyMs} ms`:result.error||`HTTP ${result.status}`}</li>)}</ul>}
    <button onClick={() => void request<unknown[]>("logs", { id: p.meta.id }).then(setLogs).catch(e => setError(pluginError(e)))}>读取运行日志</button>
    {!!logs.length && <pre className="hpp-logs">{logs.map(x => JSON.stringify(x)).join("\n")}</pre>}{children}</div>
  </div>;
}
function SettingInput({ setting: s, value, disabled, onChange }: { setting: Setting; value: unknown; disabled: boolean; onChange: (v: unknown) => void }) {
  const id = `setting-${s.key}`;
  if (s.type === "switch") return <input id={id} type="checkbox" disabled={disabled} checked={value === true} onChange={e => onChange(e.target.checked)} />;
  if (s.type === "select") return <select id={id} disabled={disabled} value={String(value ?? "")} onChange={e => onChange(e.target.value)}>{s.options?.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}</select>;
  return <input id={id} type={s.type === "number" ? "number" : s.type === "password" ? "password" : "text"} disabled={disabled} placeholder={s.placeholder} min={s.min} max={s.max} value={String(value ?? "")} onChange={e => onChange(s.type === "number" ? Number(e.target.value) : e.target.value)} />;
}
