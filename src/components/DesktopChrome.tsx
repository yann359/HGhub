import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useStore, type Tab } from "../store";
import { APP_VERSION } from "../environment";
import {
  IPin, IClose, IGrid, IGridF, IHome, IHomeF, IMinus, IMoon, IRestore, ISearch, ISettings, ISidebar, ICpu,
  ISquare, ISun, IUser, IUserF, Logo,
} from "../icons";

const appWindow = () => getCurrentWindow();

export function PlaybackWindowActions() {
  const { platform } = useStore();
  const [pinned, setPinned] = useState(false), [pending, setPending] = useState(false), [error, setError] = useState("");
  useEffect(() => { let live = true; void appWindow().isAlwaysOnTop().then(v => { if(live) setPinned(v); }).catch(()=>{}); return()=>{live=false;}; }, []);
  const togglePin = async () => { setPending(true); setError(""); try { await appWindow().setAlwaysOnTop(!pinned); setPinned(!pinned); } catch { setError("窗口置顶设置失败，请重试"); } finally { setPending(false); } };
  return <div className="win-caption-buttons hg-watch-window-actions">
    {error&&<span role="status" className="hg-window-error">{error}</span>}
    <button aria-label={pinned?"取消窗口置顶":"窗口置顶"} aria-pressed={pinned} disabled={pending} onClick={()=>void togglePin()}><IPin size={17}/></button>
    {platform !== "macos" && <>
    <button aria-label="最小化" onClick={() => appWindow().minimize().catch(() => {})}><IMinus size={16}/></button>
    <button aria-label="最大化或还原" onClick={() => appWindow().toggleMaximize().catch(() => {})}><ISquare size={14}/></button>
    <button className="win-close" aria-label="关闭窗口" onClick={() => appWindow().close().catch(() => {})}><IClose size={17}/></button></>}
  </div>;
}

export function DesktopTitleBar({ collapsed, onToggle }: { collapsed: boolean; onToggle: () => void }) {
  const { theme, setTheme, setTab, platform } = useStore();
  const [maxed, setMaxed] = useState(false);
  /* macOS 原生全屏：红绿灯被系统隐藏，标题栏左侧 84px 让位要收回（见 index.css） */
  const [nativeFs, setNativeFs] = useState(false);

  useEffect(() => {
    let un: (() => void) | undefined;
    const sync = () => {
      appWindow().isMaximized().then(setMaxed).catch(() => {});
      appWindow().isFullscreen().then(setNativeFs).catch(() => {});
    };
    sync();
    appWindow().onResized(sync)
      .then((f: () => void) => { un = f; }).catch(() => {});
    return () => { un?.(); };
  }, []);

  return (
    <header className={`win-titlebar ${nativeFs ? "is-native-fs" : ""}`} data-tauri-drag-region
      onDoubleClick={(e) => { if (!(e.target as HTMLElement).closest("button")) appWindow().toggleMaximize().catch(() => {}); }}>
      <div className="win-wordmark" data-tauri-drag-region>
        <Logo size={24} />
        <span>HG<span className="win-wordmark-light">hub</span></span>
      </div>
      <button className="win-caption-tool" aria-label={collapsed ? "展开侧栏" : "收起侧栏"} onClick={onToggle}>
        <ISidebar size={17} strokeWidth={1.7} />
      </button>
      <div className="win-drag-region" data-tauri-drag-region />
      <button className="win-caption-tool" aria-label="切换外观" onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>
        {theme === "dark" ? <ISun size={17} strokeWidth={1.7} /> : <IMoon size={17} strokeWidth={1.7} />}
      </button>
      <button className="win-caption-tool" aria-label="搜索" onClick={() => setTab("search")}>
        <ISearch size={17} strokeWidth={1.7} />
      </button>
      {/* macOS 用系统自带的红绿灯（titleBarStyle: Overlay），不画自绘按钮 */}
      {platform !== "macos" && (
        <div className="win-caption-buttons">
          <button aria-label="最小化" onClick={() => appWindow().minimize().catch(() => {})}><IMinus size={15} strokeWidth={1.5} /></button>
          <button aria-label={maxed ? "还原" : "最大化"} onClick={() => appWindow().toggleMaximize().catch(() => {})}>
            {maxed ? <IRestore size={14} strokeWidth={1.5} /> : <ISquare size={14} strokeWidth={1.5} />}
          </button>
          <button className="win-close" aria-label="关闭" onClick={() => appWindow().close().catch(() => {})}><IClose size={16} strokeWidth={1.5} /></button>
        </div>
      )}
    </header>
  );
}

export function DesktopRail({ collapsed }: { collapsed: boolean }) {
  const { tab, setTab, push, platform, stack } = useStore();
  /* 从侧栏进入的栈页（排行榜 / 设置）接管高亮，否则「我的」会一直亮着，看起来不像独立页面 */
  const top = stack[stack.length - 1];
  const railPage = top?.kind === "settings" ? "settings" : top?.kind === "plugins" ? "plugins" : null;
  const onTab = (key: Tab | "search") => !railPage && tab === key;
  const items: { key: Tab; label: string; Icon: typeof IHome; Filled: typeof IHomeF }[] = [
    { key: "home", label: "首页", Icon: IHome, Filled: IHomeF },
    { key: "category", label: "分类", Icon: IGrid, Filled: IGridF },
  ];
  return (
    <nav className={`win-sidebar ${collapsed ? "is-compact" : ""}`} aria-label="主导航">
      <div className="win-sidebar-inner">
        <button className={`win-search-launch ${onTab("search") ? "is-selected" : ""}`}
          onClick={() => setTab("search")} aria-label="搜索">
          <ISearch size={19} strokeWidth={1.7} />
          <span className="win-nav-copy">搜索你感兴趣的</span>
          <kbd className="win-nav-key">/</kbd>
        </button>
        <div className="win-nav-items">
          {items.map(({ key, label, Icon, Filled }) => (
            <button key={key} aria-current={onTab(key) ? "page" : undefined}
              className={`win-nav-item ${onTab(key) ? "is-selected" : ""}`}
              onClick={() => setTab(key)}>
              <span className="win-nav-symbol">{onTab(key) ? <Filled size={21} /> : <Icon size={21} strokeWidth={1.7} />}</span>
              <span className="win-nav-copy">{label}</span>
            </button>
          ))}
          <button className={`win-nav-item ${railPage === "plugins" ? "is-selected" : ""}`}
            aria-current={railPage === "plugins" ? "page" : undefined}
            onClick={() => { if (railPage !== "plugins") push({ kind: "plugins" }); }}>
            <span className="win-nav-symbol"><ICpu size={21} strokeWidth={1.7} /></span>
            <span className="win-nav-copy">插件中心</span>
          </button>
          <button className={`win-nav-item ${railPage === "settings" ? "is-selected" : ""}`}
            aria-current={railPage === "settings" ? "page" : undefined}
            onClick={() => { if (railPage !== "settings") push({ kind: "settings" }); }}>
            <span className="win-nav-symbol"><ISettings size={21} strokeWidth={1.7} /></span>
            <span className="win-nav-copy">设置</span>
          </button>
        </div>
        <div className="win-sidebar-bottom">
          <button aria-label="我的" className={`win-account ${onTab("profile") ? "is-selected" : ""}`} aria-current={onTab("profile") ? "page" : undefined} onClick={() => setTab("profile")}>
            <IUser size={25}/>
            <span className="win-account-copy">
              <strong>我的片库</strong>
              <small>收藏与观看进度</small>
            </span>
          </button>
          <span className="win-sidebar-version">HGhub for {platform === "macos" ? "macOS" : platform === "linux" ? "Linux" : "Windows"} {APP_VERSION}</span>
        </div>
      </div>
    </nav>
  );
}
