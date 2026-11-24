import { isMobile } from "../environment";
import { useEffect, useRef, useState } from "react";
import { useHost } from "./HostContext";
import { pluginError, type InstalledPlugin } from "./client";
import { usePluginRequests } from "./usePluginRequests";
import type { Category, Filter, FilterOption, MediaItem, Page, Section } from "./protocol";
import MediaCards, {MediaRow} from "./MediaCards";
import type { OpenSection } from "./SectionPage";
import { mergeSearch, type SearchHit } from "./searchModel";
import { PageLoadMore, usePagedPlugin } from "./usePagedPlugin";
import SearchPosters from "./SearchPosters";
import { CAT_ICONS, IGrid, IChevR, IBack, ISearch, IClose, ISettings, IFireF } from "../icons";

// Cache fulfilled metadata only. A view owns its in-flight request and may cancel it.
const categoryTrees = new Map<string, { revision: string; tree: Category[] }>();

export type OpenMedia = (pluginId:string,mediaId:string)=>void;
export function BrowseFrame({title,children,tools}:{title:string;children:React.ReactNode;tools?:React.ReactNode}) {
  return <div className="hpp-page"><header className="hpp-header"><div><h1>{title}</h1></div><div className="hpp-header-tools">{tools}</div></header><div className="hpp-scroll"><div className="hpp-content">{children}</div></div></div>;
}
export function Notice({error}:{error:string}){return error?<p role="alert" className="hpp-error">{error}</p>:null;}
export function EmptySources({manage}:{manage:()=>void}) {return <div className="hpp-empty"><h2>从一个插件开始</h2><p>导入你选择的数据源，发现影视、短剧与更多内容。</p><button className="primary" onClick={manage}>管理插件</button></div>;}
function Scope({kind,plugins}:{kind:"home"|"search";plugins:InstalledPlugin[]}) {
  const {prefs,setPrefs}=useHost(),mode=kind==="home"?prefs.homeMode:prefs.searchMode,ids=kind==="home"?prefs.homeIds:prefs.searchIds;
  const update=(mode:string,ids:string[])=>setPrefs(kind==="home"?{homeMode:mode as typeof prefs.homeMode,homeIds:ids}:{searchMode:mode as typeof prefs.searchMode,searchIds:ids});
  return <div className="hpp-scope"><div className="refined-scope-segments" aria-label="搜索范围"><span>搜索范围</span>{([["all","综合"],["single","单个来源"],["multiple","多个来源"]] as const).map(([value,label])=><button key={value} aria-pressed={mode===value} onClick={()=>update(value,ids.length?ids:plugins.slice(0,1).map(p=>p.meta.id))}>{label}</button>)}</div><label className="refined-scope-select">{kind==="home"?"首页来源":"搜索范围"}<select aria-label={kind==="home"?"首页来源":"搜索范围"} value={mode} onChange={e=>update(e.target.value,ids.length?ids:plugins.slice(0,1).map(p=>p.meta.id))}><option value="all">综合</option><option value="single">单个插件</option><option value="multiple">多个插件</option></select></label>{mode!=="all"&&plugins.map(p=><label key={p.meta.id}><input type={mode==="single"?"radio":"checkbox"} name={`${kind}-scope`} checked={ids.includes(p.meta.id)} onChange={e=>update(mode,mode==="single"?[p.meta.id]:e.target.checked?[...ids,p.meta.id]:ids.filter(id=>id!==p.meta.id))}/>{p.meta.name}</label>)}</div>;
}
export function PluginRecommendations({plugin,open,openSection,onBack}:{plugin:InstalledPlugin;open:OpenMedia;openSection:OpenSection;onBack:()=>void}) {
  const [refresh,setRefresh]=useState(0);
  return <BrowseFrame title={plugin.meta.name + " · 推荐分区"} tools={<><button onClick={()=>setRefresh(n=>n+1)}>刷新推荐</button><button onClick={onBack}>返回</button></>}><HomeSource plugin={plugin} open={open} openSection={openSection} refreshToken={refresh}/></BrowseFrame>;
}
function HomeSource({plugin:p,open,openSection,refreshToken}:{plugin:InstalledPlugin;open:OpenMedia;openSection:OpenSection;refreshToken:number}) {
  const list=usePagedPlugin<Section>({plugin:p,method:"home.getSections",limit:20}),lastRefresh=useRef(refreshToken);
  useEffect(()=>{if(lastRefresh.current!==refreshToken){lastRefresh.current=refreshToken;list.refresh();}},[refreshToken]);
  return <section className="hpp-home-source" aria-label={p.meta.name+"的推荐"}><Notice error={list.error||list.warning}/>{list.error&&<button onClick={list.retry}>重试 {p.meta.name}</button>}{list.busy&&!list.page&&<div className="hpp-skeleton" aria-label="正在加载推荐"/>}
    {list.page?.items.filter(s=>s.items.length>0).map(s=><HomeSection key={s.id} plugin={p} section={s} open={open} openSection={openSection}/>)}
    {list.page?.next&&<PageLoadMore next={list.page.next} busy={list.busy} error={list.error} more={list.more} label={"更多推荐分区 · "+p.meta.name}/>}
  </section>;
}
function HomeSection({plugin:p,section:s,open,openSection}:{plugin:InstalledPlugin;section:Section;open:OpenMedia;openSection:OpenSection}) {
  return <section className={`hpp-section hpp-layout-${s.layout}`}><div className="hpp-section-title"><h2>{s.title||"推荐"}<small>{p.meta.name}</small></h2>{s.more&&p.methods.includes("home.listSection")&&<button onClick={()=>openSection(p.meta.id,s.id,s.title)}>查看更多</button>}</div><MediaCards pluginId={p.meta.id} items={s.items} landscape={s.layout==="banner"||s.imageAspect==="landscape"} open={i=>open(p.meta.id,i.id)}/></section>;
}
export function FilterControls({filters,values,change}:{filters:Filter[];values:Record<string,string[]>;change:(v:Record<string,string[]>)=>void}) {
  return <div className="hpp-filters browse-filters">{filters.map(f=><fieldset key={f.id}><legend>{f.label}</legend>{f.options.map(o=><label key={o.id}><input type={f.multiple?"checkbox":"radio"} name={`filter-${f.id}`} checked={(values[f.id]||[]).includes(o.id)} onChange={e=>change({...values,[f.id]:f.multiple?e.target.checked?[...(values[f.id]||[]),o.id]:(values[f.id]||[]).filter(id=>id!==o.id):[o.id]})}/>{o.label}</label>)}<button type="button" aria-pressed={!values[f.id]?.length} onClick={()=>change({...values,[f.id]:[]})}>不限</button></fieldset>)}</div>;
}
export function PluginCategory({open,manage,openContent}:{open:OpenMedia;manage:()=>void;openContent?:(kind:"live"|"articles"|"feed"|"recommendations",pluginId:string)=>void}) {
  const {sources}=useHost(),available=sources.filter(p=>["catalog.getCategories","home.getSections","live.resolve","articles.list","feed.list"].some(m=>p.methods.includes(m))),[selected,setSelected]=useState("");
  const current=available.find(p=>p.meta.id===selected)||available[0];
  return <div className={"hpp-page hpp-category-page refined-page"+(isMobile?" refined-mobile":"")}><header className="shrink-0 px-4 pb-2" style={{paddingTop:"calc(var(--sat,0px) + 14px)"}}><h1 className="text-[24px] font-bold text-on leading-none">分类</h1></header><CategorySourceTabs plugins={available} selectedId={current?.meta.id} select={setSelected}/><div className="hpp-scroll"><div className="hpp-content">
    {current && openContent && <nav className="hpp-content-entries" aria-label="其他内容">{([['recommendations','home.getSections','推荐分区'],['live','live.resolve','直播'],['articles','articles.list','图文'],['feed','feed.list','信息流']] as const).filter(([,method])=>current.methods.includes(method)).map(([kind,,label])=><button key={kind} aria-label={label+" · "+current.meta.name} onClick={()=>openContent(kind,current.meta.id)}><span className="category-entry-icon"><IGrid size={18}/></span><span><strong>{label}</strong><small>{current.meta.name}</small></span><IChevR size={16}/></button>)}</nav>}
    {current?.methods.includes("catalog.getCategories")?<CategoryOverview key={current.meta.id+current.revision} plugin={current} open={open}/>:!current?<EmptySources manage={manage}/>:<p className="hpp-muted">选择上方内容入口开始浏览。</p>}
  </div></div></div>;
}
function CategorySourceTabs({plugins,selectedId,select}:{plugins:InstalledPlugin[];selectedId?:string;select:(id:string)=>void}) {
  const strip=useRef<HTMLElement>(null), [edges,setEdges]=useState({left:false,right:false});
  useEffect(()=>{
    if(isMobile)return;
    const el=strip.current!;
    const measure=()=>setEdges({left:el.scrollLeft>1,right:el.scrollLeft+el.clientWidth<el.scrollWidth-1});
    const wheel=(event:WheelEvent)=>{
      if(event.ctrlKey||Math.abs(event.deltaX)>=Math.abs(event.deltaY)||el.scrollWidth<=el.clientWidth)return;
      event.preventDefault();
      el.scrollLeft+=event.deltaY*(event.deltaMode===1?16:event.deltaMode===2?el.clientWidth:1);
    };
    const observer=new ResizeObserver(measure);
    observer.observe(el);for(const child of el.children)observer.observe(child);
    el.addEventListener("scroll",measure);el.addEventListener("wheel",wheel,{passive:false});measure();
    return()=>{observer.disconnect();el.removeEventListener("scroll",measure);el.removeEventListener("wheel",wheel);};
  },[plugins.map(p=>p.meta.id).join("|")]);
  useEffect(()=>{strip.current?.querySelector('[aria-pressed="true"]')?.scrollIntoView({block:"nearest",inline:"nearest"});},[selectedId]);
  const nav=<nav ref={strip} className="hpp-provider-tabs" aria-label="分类片源">{plugins.map(p=><button aria-label={p.meta.name} aria-pressed={selectedId===p.meta.id} className={selectedId===p.meta.id?"selected":""} key={p.meta.id} onClick={()=>select(p.meta.id)}><span className="category-source-glyph" aria-hidden="true">{p.meta.name.slice(0,1)}</span>{p.meta.name}</button>)}</nav>;
  if(isMobile)return nav;
  const move=(direction:number)=>strip.current!.scrollBy({left:direction*strip.current!.clientWidth*.8,behavior:window.matchMedia("(prefers-reduced-motion: reduce)").matches?"instant":"smooth"});
  return <div className="category-source-bar" data-overflow={edges.left||edges.right}><button className="category-scroll-arrow" aria-label="向前滚动片源" disabled={!edges.left} onClick={()=>move(-1)}><IBack size={18}/></button>{nav}<button className="category-scroll-arrow" aria-label="向后滚动片源" disabled={!edges.right} onClick={()=>move(1)}><IChevR size={18}/></button></div>;
}
function CategoryOverview({plugin,open}:{plugin:InstalledPlugin;open:OpenMedia}) {
  const request=usePluginRequests(),[tree,setTree]=useState<Category[]>([]),[selected,setSelected]=useState<Category|null>(null),[error,setError]=useState(""),[busy,setBusy]=useState(true),[attempt,setAttempt]=useState(0);
  useEffect(()=>{let live=true;setBusy(true);setError("");const cached=categoryTrees.get(plugin.meta.id);const load=cached?.revision===plugin.revision?Promise.resolve(cached.tree):request<Category[]>("call",{id:plugin.meta.id,method:"catalog.getCategories",args:{}});void load.then(tree=>{if(live){categoryTrees.set(plugin.meta.id,{revision:plugin.revision,tree});setTree(tree);}}).catch(e=>{if(live)setError(pluginError(e));}).finally(()=>{if(live)setBusy(false);});return()=>{live=false;request.cancelAll();};},[plugin.meta.id,plugin.revision,attempt,request]);
  const content=selected?<><button className="hpp-back" onClick={()=>setSelected(null)}><IBack size={17}/>全部分类</button><h2>{selected.label}</h2><CategorySource key={selected.id} plugin={plugin} open={open} initialCategory={selected.id}/></>:<><Notice error={error}/>{error&&<button onClick={()=>setAttempt(v=>v+1)}>重试分类</button>}{busy?<div className="hpp-skeleton" aria-label="正在加载分类"/>:tree.map(category=><CategoryPreview key={category.id} plugin={plugin} category={category} open={open} all={()=>setSelected(category)}/>)}{!busy&&!error&&!tree.length&&<div className="hpp-empty"><IGrid size={36}/><h2>这里还没有分类</h2><p>片源暂未提供分类内容。</p></div>}{!!tree.length&&<p className="hpp-end-note">— 已展示全部分类 —</p>}</>;
  return <div className="refined-category-layout"><aside className="refined-category-directory"><h2>分类目录</h2><nav aria-label="分类目录"><button aria-pressed={!selected} onClick={()=>setSelected(null)}><IGrid size={18}/>全部概览</button>{tree.map(c=><button key={c.id} aria-pressed={selected?.id===c.id} onClick={()=>setSelected(c)}><span><strong>{c.label}</strong><small>{c.children?.map(child=>child.label).join(" · ")}</small></span><IChevR size={14}/></button>)}</nav></aside><div className="refined-category-results">{content}</div></div>;
}
function CategoryPreview({plugin,category,open,all}:{plugin:InstalledPlugin;category:Category;open:OpenMedia;all:()=>void}) {
  const list=usePagedPlugin<MediaItem>({plugin,method:"catalog.list",args:{categoryId:category.children?.[0]?.id||category.id},limit:8});
  const Icon=CAT_ICONS[category.label]||IGrid;
  return <section className="hpp-category-preview legacy-category-preview mb-5 a-rise" aria-label={category.label}>
    <button onClick={all} className="legacy-category-heading w-full flex items-center gap-2.5 mb-2.5 text-left pressable group"><span className="w-[36px] h-[36px] rounded-(--r-sm) tonal flex items-center justify-center shrink-0 text-primary"><Icon size={19} strokeWidth={1.9}/></span><span className="flex-1 min-w-0"><span className="block text-[15.5px] font-semibold text-on leading-tight">{category.label}</span><span className="block text-[10.5px] text-mut mt-0.5 truncate">{category.children?.map(c=>c.label).join(" · ")||plugin.meta.name}</span></span><span className="flex items-center gap-0.5 text-[11.5px] text-mut">全部<IChevR size={14}/></span></button>
    <Notice error={list.error}/>{list.error&&<button onClick={list.retry}>重试 {category.label}</button>}
    {list.busy&&!list.page?<div className="hpp-preview-skeleton">{Array.from({length:6},(_,i)=><span className="hpp-skeleton" key={i}/>)}</div>:<div className="hpp-preview-strip scroll-x flex gap-2.5 snap-x"><MediaRow pluginId={plugin.meta.id} items={list.page?.items||[]} open={item=>open(plugin.meta.id,item.id)}/></div>}
    {!list.busy&&!list.error&&!list.page?.items.length&&<p className="hpp-muted">暂无内容</p>}
  </section>;
}
function CategorySource({plugin:p,open,initialCategory}:{plugin:InstalledPlugin;open:OpenMedia;initialCategory?:string}) {
  const metadata=usePluginRequests(),[tree,setTree]=useState<Category[]>([]),[category,setCategory]=useState(""),[error,setError]=useState(""),[filters,setFilters]=useState<Filter[]>([]),[sorts,setSorts]=useState<FilterOption[]>([]),[values,setValues]=useState<Record<string,string[]>>({}),[sort,setSort]=useState("");
  const list=usePagedPlugin<MediaItem>({plugin:p,method:"catalog.list",args:{categoryId:category,filters:values,sort:sort||undefined},enabled:!!category});
  useEffect(()=>{let live=true;const cached=categoryTrees.get(p.meta.id);const load=cached?.revision===p.revision?Promise.resolve(cached.tree):metadata<Category[]>("call",{id:p.meta.id,method:"catalog.getCategories",args:{}});void load.then(r=>{if(live){categoryTrees.set(p.meta.id,{revision:p.revision,tree:r});setTree(r);const selected=r.find(c=>c.id===initialCategory);setCategory(selected?.children?.[0]?.id||initialCategory||r[0]?.children?.[0]?.id||r[0]?.id||"");}}).catch(e=>{if(live)setError(pluginError(e));});return()=>{live=false;};},[p.meta.id,p.revision,metadata]);
  useEffect(()=>{let live=true;setValues({});setSort("");const c=tree.flatMap(c=>[c,...c.children||[]]).find(c=>c.id===category);setFilters(c?.filters||[]);setSorts(c?.sorts||[]);if(category&&p.methods.includes("catalog.getFilters"))void metadata<{filters?:Filter[];sorts?:FilterOption[]}>("call",{id:p.meta.id,method:"catalog.getFilters",args:{categoryId:category}}).then(r=>{if(live){setFilters(r.filters||[]);setSorts(r.sorts||[]);}}).catch(e=>{if(live)setError(pluginError(e));});return()=>{live=false;};},[category]);
  return <><div className="hpp-category-tree">{tree.map(c=><div key={c.id}><button aria-pressed={category===c.id} className={category===c.id?"selected":""} onClick={()=>setCategory(c.id)}>{c.label}</button>{c.children?.map(child=><button key={child.id} aria-pressed={category===child.id} className={category===child.id?"selected":""} onClick={()=>setCategory(child.id)}>{child.label}</button>)}</div>)}</div><FilterControls filters={filters} values={values} change={setValues}/>{!!sorts.length&&<label>排序 <select aria-label="分类排序" value={sort} onChange={e=>setSort(e.target.value)}><option value="">默认</option>{sorts.map(s=><option value={s.id} key={s.id}>{s.label}</option>)}</select></label>}<Notice error={error||list.error||list.warning}/>{list.error&&<button onClick={list.retry}>重试分类</button>}{list.busy&&!list.page?<div className="hpp-skeleton"/>:<MediaCards pluginId={p.meta.id} items={list.page?.items||[]} open={i=>open(p.meta.id,i.id)}/>}<PageLoadMore next={list.page?.next} busy={list.busy} error={list.error} more={list.more}/></>;
}
export function PluginSearch({open,manage}:{open:OpenMedia;manage:()=>void}) {
  const {sources,prefs}=useHost(),request=usePluginRequests(),metadata=usePluginRequests(),suggestRequest=usePluginRequests(),[query,setQuery]=useState(""),[submitted,setSubmitted]=useState(""),[pages,setPages]=useState<Record<string,Page<MediaItem>>>({}),[errors,setErrors]=useState<Record<string,string>>({}),[busy,setBusy]=useState<Record<string,boolean>>({}),[tab,setTab]=useState("all"),[choice,setChoice]=useState<SearchHit[]|null>(null),[suggest,setSuggest]=useState<string[]>([]),[trending,setTrending]=useState<Record<string,string[]>>({}),[filters,setFilters]=useState<Filter[]>([]),[values,setValues]=useState<Record<string,string[]>>({}),epoch=useRef(0);
  const locks=useRef(new Map<string,number>()), consumed=useRef(new Map<string,Set<string>>()), failedCursors=useRef(new Map<string,string|undefined>()), searchFilters=useRef<Record<string,string[]>>({});
  const [warnings,setWarnings]=useState<Record<string,string>>({});
  const [viewMode,setViewMode]=useState<"grid"|"detail">("grid");
  const [showScope,setShowScope]=useState(false),inputRef=useRef<HTMLInputElement>(null);
  const [history,setHistory]=useState<string[]>(()=>{try{const saved=JSON.parse(localStorage.getItem("hpp.searchHistory")||"[]");return Array.isArray(saved)?[...new Set<string>(saved.filter((v:unknown):v is string=>typeof v==="string"&&!!v.trim()))].slice(0,20):[];}catch{return [];}});
  useEffect(()=>{try{localStorage.setItem("hpp.searchHistory",JSON.stringify(history));}catch{/* Search remains usable if storage is unavailable. */}},[history]);
  useEffect(()=>{if(!/Android|iPhone|iPad/i.test(navigator.userAgent))inputRef.current?.focus();},[]);

  const available=sources.filter(p=>p.methods.includes("search.search")),chosen=prefs.searchMode==="all"?available:prefs.searchIds.flatMap(id=>available.find(p=>p.meta.id===id)||[]).slice(0,prefs.searchMode==="single"?1:undefined),ids=chosen.map(p=>p.meta.id+p.revision).join("|");
  useEffect(()=>{let live=true;setTrending({});for(const p of chosen)if(p.methods.includes("search.trending"))void metadata<string[]>("call",{id:p.meta.id,method:"search.trending",args:{}}).then(r=>{if(live)setTrending(old=>({...old,[p.meta.id]:r}));}).catch(()=>{});return()=>{live=false;};},[ids]);
  useEffect(()=>{let live=true;setFilters([]);setValues({});const p=prefs.searchMode==="single"?chosen[0]:undefined;if(p?.methods.includes("search.getFilters"))void metadata<Filter[]>("call",{id:p.meta.id,method:"search.getFilters",args:{}}).then(r=>{if(live)setFilters(r);}).catch(()=>{});return()=>{live=false;};},[ids,prefs.searchMode]);
  useEffect(()=>{let live=true;setSuggest([]);const p=prefs.searchMode==="single"?chosen[0]:undefined;const timer=window.setTimeout(()=>{if(query.trim()&&p?.methods.includes("search.suggest"))void suggestRequest<string[]>("call",{id:p.meta.id,method:"search.suggest",args:{query}}).then(r=>{if(live)setSuggest(r);}).catch(()=>{});},300);return()=>{live=false;clearTimeout(timer);suggestRequest.cancelAll();};},[query,ids,prefs.searchMode]);
  useEffect(()=>{request.cancelAll();epoch.current++;setPages({});setErrors({});setBusy({});setSubmitted("");setTab(prefs.searchMode==="all"?"all":chosen[0]?.meta.id||"");return()=>{epoch.current++;request.cancelAll();};},[ids,prefs.searchMode]);
  const load=async(p:InstalledPlugin,q:string,n:number,cursor?:string)=>{
    if(n!==epoch.current||locks.current.get(p.meta.id)===n||cursor!==undefined&&consumed.current.get(p.meta.id)?.has(cursor))return;
    locks.current.set(p.meta.id,n);failedCursors.current.set(p.meta.id,cursor);
    setBusy(old=>({...old,[p.meta.id]:true}));setErrors(old=>({...old,[p.meta.id]:""}));
    try{
      const r=await request<Page<MediaItem>>("call",{id:p.meta.id,method:"search.search",args:{query:q,filters:searchFilters.current,cursor,limit:40}});
      if(n!==epoch.current)return;
      const seen=cursor===undefined?new Set<string>():consumed.current.get(p.meta.id)||new Set<string>();
      if(cursor!==undefined)seen.add(cursor);consumed.current.set(p.meta.id,seen);
      const repeated=!!r.next&&seen.has(r.next);
      setWarnings(old=>({...old,[p.meta.id]:repeated?"来源返回了重复分页游标，已停止继续加载。":""}));
      setPages(old=>{
        const previous=cursor===undefined?[]:old[p.meta.id]?.items||[],ids=new Set(previous.map(i=>i.id));
        return {...old,[p.meta.id]:{...r,next:repeated?null:r.next,items:[...previous,...r.items.filter(i=>{if(ids.has(i.id))return false;ids.add(i.id);return true;})]}};
      });
    }catch(e){if(n===epoch.current&&(e as {code?:string}).code!=="CANCELLED")setErrors(old=>({...old,[p.meta.id]:pluginError(e)}));}
    finally{if(locks.current.get(p.meta.id)===n)locks.current.delete(p.meta.id);if(n===epoch.current)setBusy(old=>({...old,[p.meta.id]:false}));}
  };
  const search=(q=query)=>{q=q.trim();if(!q||!chosen.length)return;setQuery(q);setHistory(old=>[q,...old.filter(h=>h!==q)].slice(0,20));request.cancelAll();const n=++epoch.current;locks.current.clear();consumed.current.clear();failedCursors.current.clear();searchFilters.current=values;setSubmitted(q);setPages({});setErrors({});setWarnings({});setBusy({});setChoice(null);setSuggest([]);for(const p of chosen)void load(p,q,n);};
  useEffect(()=>{if(submitted)search(submitted);},[JSON.stringify(values)]);
  const hits=chosen.flatMap(p=>(pages[p.meta.id]?.items||[]).map(item=>({pluginId:p.meta.id,item}))),groups=mergeSearch(hits);
  const visibleGroups=tab==="all"?groups:hits.filter(h=>h.pluginId===tab).map(h=>[h]);
  const activeSources=tab==="all"?chosen:chosen.filter(p=>p.meta.id===tab);
  const loading=activeSources.some(p=>busy[p.meta.id]);
  const clear=()=>{request.cancelAll();suggestRequest.cancelAll();epoch.current++;setQuery("");setSubmitted("");setPages({});setErrors({});setWarnings({});setBusy({});setChoice(null);setSuggest([]);inputRef.current?.focus();};
  const openGroup=(group:SearchHit[])=>group.length>1?setChoice(group):open(group[0].pluginId,group[0].item.id);
  return <div className={"refined-page refined-search legacy-search-page absolute inset-0 bg-bg flex flex-col"+(isMobile?" refined-mobile":"")}>
    <header className="refined-search-heading"><h1>聚合搜索</h1><span>{chosen.length} 个参与搜索的来源</span></header>
    <div className="refined-search-console shrink-0 px-4 pb-3 flex items-center gap-2.5" style={{paddingTop:"calc(var(--sat, 0px) + 12px)"}}>
      <form className="hpp-search legacy-search-form flex-1 flex items-center gap-2 h-11 rounded-(--r-ctl) bg-s3 px-3.5 focus-within:bg-s4 transition-colors" onSubmit={e=>{e.preventDefault();search();inputRef.current?.blur();}}>
        <button type="submit" aria-label="搜索" disabled={!query.trim()||!chosen.length} className="text-mut shrink-0 pressable"><ISearch size={17}/></button>
        <input ref={inputRef} aria-label="搜索所有插件" placeholder="搜索作品、作者、题材…" value={query} onChange={e=>setQuery(e.target.value)} className="flex-1 min-w-0 bg-transparent text-[13.5px] text-on placeholder-dim outline-none"/>
        {query&&<button type="button" aria-label="清除搜索" onClick={clear} className="w-5 h-5 rounded-full bg-s4 flex items-center justify-center text-mut pressable"><IClose size={11}/></button>}
        <button className="refined-search-submit" type="submit" disabled={!query.trim()||!chosen.length}>搜索片源</button>
      </form>
      <button type="button" aria-label="搜索来源与筛选" aria-expanded={showScope} onClick={()=>setShowScope(v=>!v)} className="w-11 h-11 shrink-0 rounded-(--r-sm) flex items-center justify-center bg-primary text-on-primary pressable elev-2"><ISettings size={19}/></button>
    </div>
    {isMobile&&<button className="search-scope-trigger" aria-expanded={showScope} onClick={()=>setShowScope(v=>!v)}><ISettings size={15}/><span>搜索范围</span><strong>{prefs.searchMode==="all"?"综合 · 全部 "+chosen.length+" 个来源":"已选 "+chosen.length+" 个来源"}</strong><IChevR size={14}/></button>}
    <div className="refined-search-scroll flex-1 min-h-0 scroll-y px-4 pb-6">
      <div className={`legacy-search-options refined-scope ${showScope?"is-open":""}`}><Scope kind="search" plugins={available}/><FilterControls filters={filters} values={values} change={setValues}/><button onClick={manage} className="text-primary text-xs pressable">管理插件</button></div>
      {!!suggest.length&&query!==submitted&&<div className="flex flex-wrap gap-2 mb-3" aria-label="搜索建议">{suggest.map(s=><button className="px-3 h-8 rounded-(--r-ctl) bg-s3 text-xs text-on pressable" key={s} onClick={()=>search(s)}>{s}</button>)}</div>}
      {!submitted?<div className="refined-search-idle"><div className="refined-search-recents">
        {!!history.length&&<section className="mt-1 a-rise" aria-label="搜索历史">
          <div className="flex items-center justify-between"><span className="text-[13px] font-bold text-on">搜索历史</span><button onClick={()=>setHistory([])} className="text-[11px] text-dim pressable">清空</button></div>
          <div className="flex flex-wrap gap-2 mt-2.5">{history.map(h=><button key={h} onClick={()=>search(h)} className="px-3.5 h-8 rounded-(--r-ctl) bg-s3 text-[12px] text-on/80 pressable">{h}</button>)}</div>
        </section>}
        {chosen.filter(p=>trending[p.meta.id]?.length).map(p=><section key={p.meta.id} className="mt-5 rounded-(--r-md) bg-s2 p-3.5 a-rise"><div className="flex items-center justify-between gap-2"><span className="flex items-center gap-1.5 text-[13px] font-semibold text-on/90"><IFireF size={15} className="text-primary"/>热门搜索</span><span className="text-[11px] text-mut">{p.meta.name}</span></div><div className="flex flex-wrap gap-2 mt-3">{trending[p.meta.id].map(s=><button key={s} onClick={()=>search(s)} className="px-3 h-7 rounded-(--r-ctl) bg-s3 text-[12px] text-on/85 pressable">{s}</button>)}</div></section>)}
        </div><aside className="refined-search-sources"><div className="refined-section-heading"><h2>参与搜索的来源</h2><button onClick={manage}>管理插件<IChevR size={14}/></button></div><div className="refined-source-list">{chosen.map(p=><div className="refined-source-row" key={p.meta.id}><span className="refined-glyph">{p.meta.name.slice(0,1)}</span><span><strong>{p.meta.name}</strong><small>v{p.meta.version}</small></span><span className="refined-source-check">✓</span></div>)}</div>
        {!available.length?<EmptySources manage={manage}/>:<p className="text-[11.5px] text-dim leading-relaxed mt-6">{chosen.length?"输入关键词后按回车搜索，也可以点搜索历史或热门词直接查看。":"请在右上角选择搜索片源。"}</p>}
      </aside></div>:<>
        {chosen.length>1&&<nav className="legacy-search-tabs flex gap-2 overflow-x-auto pb-2" aria-label="搜索结果来源"><button aria-pressed={tab==="all"} className={tab==="all"?"selected":""} onClick={()=>{setTab("all");setChoice(null);}}>全部</button>{chosen.map(p=><button key={p.meta.id} aria-pressed={tab===p.meta.id} className={tab===p.meta.id?"selected":""} onClick={()=>{setTab(p.meta.id);setChoice(null);}}>{p.meta.name}{busy[p.meta.id]?" · 加载中":` · ${pages[p.meta.id]?.items.length||0}`}</button>)}</nav>}
        <div className="refined-results-toolbar"><div className="text-[11.5px] text-mut mt-1 mb-2.5" aria-live="polite">「{submitted}」共 <span className="text-primary font-bold">{visibleGroups.length}</span> 条结果{loading?" · 搜索中…":""}</div><div className="refined-view-toggle" aria-label="结果显示方式"><button aria-pressed={viewMode==="grid"} onClick={()=>setViewMode("grid")}>海报网格</button><button aria-pressed={viewMode==="detail"} onClick={()=>setViewMode("detail")}>详细卡片</button></div></div>
        {choice?<div className="hpp-source-choice legacy-search-options"><div className="flex items-center justify-between mb-3"><h2 className="text-sm font-bold">选择内容来源</h2><button onClick={()=>setChoice(null)} className="text-primary text-xs">返回搜索</button></div>{choice.map(h=><button className="block w-full text-left p-3 mb-2 bg-s3 rounded-(--r-ctl) text-xs" key={h.pluginId+h.item.id} onClick={()=>open(h.pluginId,h.item.id)}>{sources.find(p=>p.meta.id===h.pluginId)?.meta.name} · {h.item.title}</button>)}</div>:
          <SearchPosters viewMode={viewMode} groups={visibleGroups} names={Object.fromEntries(sources.map(p=>[p.meta.id,p.meta.name]))} open={openGroup}/>}
        {!visibleGroups.length&&(loading?<div className="flex justify-center py-12"><div className="w-8 h-8 spinner" aria-label="正在搜索"/></div>:<div className="py-12 text-center text-mut"><ISearch size={26} className="mx-auto mb-3"/><p className="text-sm">没有找到相关内容</p><p className="text-xs text-dim mt-2">换个关键词或片源试试</p></div>)}
        {activeSources.map(p=><section key={p.meta.id}><Notice error={errors[p.meta.id]||warnings[p.meta.id]||""}/>{errors[p.meta.id]&&<button className="text-primary text-xs py-3" onClick={()=>void load(p,submitted,epoch.current,failedCursors.current.get(p.meta.id))}>{tab==="all"?`重试 ${p.meta.name}`:"重试搜索"}</button>}{pages[p.meta.id]?.next&&<PageLoadMore next={pages[p.meta.id].next} busy={!!busy[p.meta.id]} error={errors[p.meta.id]||""} more={()=>void load(p,submitted,epoch.current,pages[p.meta.id].next||undefined)} label={`加载更多 · ${p.meta.name}`}/>}</section>)}
      </>}
    </div>
  </div>;
}
