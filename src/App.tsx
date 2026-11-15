import { useEffect, useRef, useState } from "react";
import { useStore, type Tab } from "./store";
import { IGrid, IGridF, IHome, IHomeF, ISearch, IUser, IUserF } from "./icons";
import { isMobile } from "./environment";
import { DesktopRail, DesktopTitleBar } from "./components/DesktopChrome";
import ResizeHandles from "./components/ResizeHandles";
import Plugins from "./pages/Plugins";
import { HostProvider, useHost } from "./plugins/HostContext";
import { BrowseFrame, PluginRecommendations, PluginCategory, PluginSearch } from "./plugins/BrowsePages";
import PluginMediaDetail from "./plugins/PluginMediaDetail";
import LibraryPage from "./plugins/LibraryPage";
import ApplicationSettings from "./plugins/ApplicationSettings";
import SectionPage from "./plugins/SectionPage";
import LivePage from "./plugins/LivePage";
import ArticlePage, { ArticleList } from "./plugins/ArticlePage";
import FeedPage from "./plugins/FeedPage";
import type { SourceStart } from "./plugins/sourceMatching";
import { pendingUpdates } from "./plugins/updateClient";
import "./plugins/plugins.css";
import "./plugins/shell.css";
import "./plugins/legacy-adapter.css";
import ImmersiveHome from "./plugins/ImmersiveHome";
import { runBack } from "./back";
import { addPluginListener, invoke } from "@tauri-apps/api/core";
export default function App() { return <HostProvider><Application/></HostProvider>; }
function Application() {
  const app=useStore(),host=useHost(),[collapsed,setCollapsed]=useState(false),top=app.stack[app.stack.length-1];
  const backRef=useRef(()=>{});
  backRef.current=()=>{if(app.fs){app.setFs(false);return;}if(document.fullscreenElement){void document.exitFullscreen();return;}if(runBack())return;if(app.stack.length)app.pop();else if(app.tab!=="home")app.setTab("home");else void invoke("plugin:app|exit",{code:0}).catch(()=>{});};
  useEffect(()=>{if(app.platform!=="android")return;let disposed=false;let listener:{unregister:()=>Promise<void>}|undefined;void addPluginListener("app","back-button",()=>backRef.current()).then(l=>{if(disposed)void l.unregister();else listener=l;}).catch(()=>{});return()=>{disposed=true;void listener?.unregister();};},[app.platform]);
  const manage=(focusId?:string)=>{host.clearAuth();app.push({kind:"plugins",focusId});};
  const open=(pluginId:string,mediaId:string)=>app.push({kind:"media",pluginId,mediaId});
  useEffect(()=>{const fn=(e:KeyboardEvent)=>{
    const target=e.target as HTMLElement;
    if(e.defaultPrevented||document.querySelector("dialog[open]")||target.closest("input,textarea,select,[contenteditable=true],[role=dialog]"))return;
    if(e.key==="Escape"){if(runBack()){e.preventDefault();return;}if(app.fs)app.setFs(false);else if(document.fullscreenElement)void document.exitFullscreen();else app.pop();}
    else if(e.key==="/"){e.preventDefault();app.setTab("search");}
    else if(!e.repeat&&["1","2","3","4"].includes(e.key))app.setTab((["home","search","category","profile"] as Tab[])[Number(e.key)-1]);
  };window.addEventListener("keydown",fn);return()=>window.removeEventListener("keydown",fn);},[app.pop,app.setTab]);
  return <div className={`absolute inset-0 flex flex-col bg-canvas win-root plat-${app.platform}`} style={isMobile?{"--sat":"max(env(safe-area-inset-top), 12px)","--sab":"max(env(safe-area-inset-bottom), 16px)"} as React.CSSProperties:undefined}>
    {!isMobile&&<DesktopTitleBar collapsed={collapsed} onToggle={()=>setCollapsed(v=>!v)}/>}
    <div className="relative flex-1 min-h-0 flex">{!isMobile&&!app.theater&&<DesktopRail collapsed={collapsed}/>}<main className={`relative flex-1 min-w-0 overflow-hidden win-main ${app.theater?"is-theater-main":""}`}>
      <div className="absolute inset-0" style={{bottom:isMobile&&!top&&!app.fs&&!app.clean?(app.platform==="ios"?"calc(82px + var(--sab, 0px))":"calc(40px + var(--sab, 0px))"):0}}>{!top&&(app.tab==="home"?<ImmersiveHome open={open} manage={manage} settings={()=>app.push({kind:"settings"})}/>:app.tab==="category"?<PluginCategory open={open} manage={manage} openContent={(kind,pluginId)=>app.push({kind,pluginId})}/>:app.tab==="search"?<PluginSearch open={open} manage={manage}/>:<LibraryPage open={open} manage={manage} settings={()=>app.push({kind:"settings"})}/>)}</div>
      {app.stack.map((page,i)=><div key={page.key} className={`absolute inset-0 bg-bg ${i===app.stack.length-1?"z-30":"invisible pointer-events-none"}`}>
        {page.kind==="plugins"?<Plugins onBack={app.pop} focusId={page.focusId}/>:page.kind==="settings"?<ApplicationSettings onBack={app.pop}/>:page.kind==="section"?<SectionRoute {...page} open={open} onBack={app.pop}/>:page.kind==="media"?<MediaPage pluginId={page.pluginId} mediaId={page.mediaId} start={page.start} onBack={app.pop} manage={manage} active={i===app.stack.length-1} onSwitch={(pluginId,mediaId,start)=>app.replace({kind:"media",pluginId,mediaId,start})}/>:<ContentRoute page={page} active={i===app.stack.length-1}/>}
      </div>)}
      {host.authPrompt&&<div className="hpp-global-auth" role="status"><span>{host.authPrompt.message}</span><button onClick={()=>manage(host.authPrompt?.id)}>前往插件登录</button><button onClick={host.clearAuth}>关闭</button></div>}
      {pendingUpdates(host.updates).length>0&&!top&&<div className="hpp-global-updates" role="status"><span>{pendingUpdates(host.updates).length} 个插件有更新{host.updates.policy.autoInstall?"，将在播放结束后安装":""}</span><button onClick={()=>manage()}>查看插件更新</button></div>}
    </main></div>
    {isMobile&&!top&&!app.fs&&!app.clean&&<TabBar/>}
    {app.platform==="windows"&&<ResizeHandles/>}
  </div>;
}
function ContentRoute({ page, active }: { page: Extract<import("./store").Page, { kind: "live" | "articles" | "article" | "feed" | "recommendations" }>; active: boolean }) {
  const host = useHost(), app = useStore(), plugin = host.sources.find(p => p.meta.id === page.pluginId);
  if (!plugin) return <BrowseFrame title="来源不可用" tools={<button onClick={app.pop}>返回</button>}><p>请重新启用该来源。</p></BrowseFrame>;
  const key = plugin.meta.id + plugin.revision;
  if (page.kind === "recommendations") return <PluginRecommendations key={key} plugin={plugin} open={(pluginId,mediaId)=>app.push({kind:"media",pluginId,mediaId})} openSection={(pluginId,sectionId,title)=>app.push({kind:"section",pluginId,sectionId,title})} onBack={app.pop}/>;
  if (page.kind === "feed") return <FeedPage key={key} plugin={plugin} active={active} onBack={app.pop} openMedia={mediaId => app.push({ kind: "media", pluginId: plugin.meta.id, mediaId })}/>;
  if (page.kind === "live") return <LivePage key={key} plugin={plugin} active={active} onBack={app.pop}/>;
  if (page.kind === "articles") return <ArticleList key={key} plugin={plugin} onBack={app.pop} open={articleId => app.push({ kind: "article", pluginId: plugin.meta.id, articleId })}/>;
  return <ArticlePage key={key + page.articleId} plugin={plugin} articleId={page.articleId} active={active} onBack={app.pop} openMedia={mediaId => app.push({ kind: "media", pluginId: plugin.meta.id, mediaId })}/>;
}
function SectionRoute({pluginId,sectionId,title,open,onBack}:{pluginId:string;sectionId:string;title?:string;open:(pluginId:string,mediaId:string)=>void;onBack:()=>void}) {
  const plugin=useHost().sources.find(p=>p.meta.id===pluginId);
  return plugin?<SectionPage key={pluginId+sectionId+plugin.revision} plugin={plugin} sectionId={sectionId} title={title} open={open} onBack={onBack}/>:<BrowseFrame title="来源不可用" tools={<button onClick={onBack}>返回</button>}><p>请启用该分区的数据源。</p></BrowseFrame>;
}
function MediaPage({pluginId,mediaId,onBack,manage,active,start,onSwitch}: {pluginId:string;mediaId:string;onBack:()=>void;manage:()=>void;active:boolean;start?:SourceStart;onSwitch:(pluginId:string,mediaId:string,start?:SourceStart)=>void}) {
  const host=useHost(),plugin=host.sources.find(p=>p.meta.id===pluginId);
  return plugin?<PluginMediaDetail key={pluginId+mediaId+plugin.revision} plugin={plugin} mediaId={mediaId} onBack={onBack} manage={manage} active={active} start={start} onSwitch={onSwitch}/>:<BrowseFrame title="来源不可用" tools={<button onClick={onBack}>返回</button>}><p>请重新导入或启用该作品的数据源。</p><button onClick={manage}>管理插件</button></BrowseFrame>;
}

function TabBar() {
  const { tab, setTab, theme, platform } = useStore();
  const items: { k: Tab; label: string }[] = [
    { k: "home", label: "首页" },
    { k: "search", label: "搜索" },
    { k: "category", label: "分类" },
    { k: "profile", label: "我的" },
  ];
  if (platform === "ios") {
    const icons = { home: [IHome, IHomeF], search: [ISearch, ISearch], category: [IGrid, IGridF], profile: [IUser, IUserF] };
    return <nav className={`ios-tabbar ${tab === "home" ? "over-video" : ""}`} aria-label="主导航">
      {items.map(({ k, label }) => {
        const Icon = icons[k][tab === k ? 1 : 0];
        return <button key={k} aria-current={tab === k ? "page" : undefined} onClick={() => setTab(k)}>
          <Icon size={23} strokeWidth={1.7}/><span>{label}</span>
        </button>;
      })}
    </nav>;
  }
  /* 细瘦纯文字分栏：只有选中项高亮，没有指示点。
     首页是沉浸视频 → 底栏压深色；搜索/分类/我的 是浅色内容页 →
     浅色主题下底栏跟着换成浅色，深色主题保持深色。 */
  const overVideo = tab === "home";
  const lightBar = !overVideo && theme === "light";
  return (
    <nav className="hg-tabbar absolute bottom-0 inset-x-0 z-30"
      style={{
        paddingBottom: "var(--sab, 0px)",
        background: lightBar ? "rgba(250, 250, 250, .97)" : overVideo ? "#000000" : "#141414",
        borderTop: lightBar ? "1px solid rgba(0,0,0,.08)" : overVideo ? "none" : "1px solid rgba(255,255,255,.07)",
        boxShadow: lightBar ? "0 -2px 14px rgba(0,0,0,.05)" : "none",
      }}>
      <div className="h-[40px] flex items-center px-1">
        {items.map(({ k, label }) => {
          const on = tab === k;
          return (
            <button key={k} onClick={() => setTab(k)} className="flex-1 h-full flex items-center justify-center pressable">
              <span className="text-[12.5px] leading-none transition-colors duration-200"
                style={{
                  fontWeight: on ? 700 : 400,
                  color: lightBar
                    ? (on ? "var(--p-primary)" : "rgba(0,0,0,.45)")
                    : (on ? "#ffffff" : "rgba(255,255,255,.6)"),
                }}>
                {label}
              </span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
