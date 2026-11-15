import React, { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { fullscreen as enterVideoFullscreen, pictureInPicture } from "../mediaCapabilities";
import { isMobile, platform } from "../environment";
import { fmtTime } from "../format";
import { useStore } from "../store";
import { Sheet } from "./ui";
import VideoSurface, { type VideoHandle } from "./VideoSurface";
import PlayerControls from "./PlayerControls";
import PlaybackLock from "./PlaybackLock";
import { setBackHandler } from "../back";
import type { PreparedMedia } from "../plugins/client";
import { IBack, IPip, IBoltF, IChevR, IChevronDown, IChevronUp, IClose, ICommentF, ICompress, IExpand, IList, ILockF, IPauseF, IPlayF, ISearch, ISettings, IStar, IStarF, IVol, IVolMute } from "../icons";

export type PlayerPanel = "detail" | "episodes" | "comments" | "playback";
export interface FeedPresentation {
  title?: string; author?: string; description?: string; score?: number; poster?: string;
  episodeLabel?: string; episodeCount?: number; favorite?: boolean; onFavorite?: () => void;
  panels?: Partial<Record<PlayerPanel, ReactNode>>; social?: ReactNode;
  onPanelChange?: (panel: PlayerPanel) => void;
  onBack?: () => void; onNext?: () => void; onPrevious?: () => void; onNextEpisode?: () => void;
  queueIndex?: number; total?: number;
}
export interface FeedProps extends FeedPresentation {
  media: PreparedMedia | null; position: number; active?: boolean; error?: string; onRetry?: () => void;
  onPosition: (t: number, duration?: number) => void; onFailure: () => void;
  onPlaying?: () => void; onStopped?: () => void; onEnded?: () => void;
}
/** Restored from 5f88442 Feed: shared desktop chrome and mobile gestures.
 * Plugin adapters own metadata, sessions and persistence; this component owns presentation. */
export default function Feed({media, position, active=true, error="", onRetry, onPosition, onFailure, onPlaying, onStopped, onEnded,
 title="", author="", description="", score, poster:posterUrl, episodeLabel="", episodeCount=0, favorite=false, onFavorite,
 panels={}, social, onPanelChange, onBack, onNext, onPrevious, onNextEpisode, queueIndex=0, total=1}: FeedProps) {
  const store=useStore(), {muted,setMuted,volume,setVolume}=store;
  const isDesktop=!isMobile, bottomInset=0;
  const boxRef=useRef<HTMLDivElement>(null), videoRef=useRef<VideoHandle>(null);
  const lastWheel=useRef(0);
  const playKey=media?.sessionId || "pending", playUrl=media?.url;
  const meta={title,author,desc:description,score,comment:undefined as number|undefined};
  const curEp=episodeLabel, index=0;
  const [hint,setHint]=useState("");
  const toast=(message:string,_kind?:string)=>setHint(message);
  useEffect(()=>{if(!hint)return;const t=window.setTimeout(()=>setHint(""),2200);return()=>window.clearTimeout(t);},[hint]);
  const [subtitle,setSubtitle]=useState(-1);
  useEffect(()=>setSubtitle(media?.playback.subtitles?.filter(t=>t.format==="vtt").findIndex(t=>t.default)??-1),[playKey]);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [vsize, setVsize] = useState({ w: 0, h: 0 });
  const [buffered, setBuffered] = useState(0);
  const [paused, setPaused] = useState(false);
  const [controlsLocked, setControlsLocked] = useState(false);
  const [rate, setRate] = useState(1);
  const [waiting, setWaiting] = useState(false);
  const [err, setErr] = useState("");
  /* 取播放地址失败后手动重试：递增即重新触发拉取 */
  const [panel, setPanel] = useState<PlayerPanel | null>(null);
  useEffect(() => { if (panel) onPanelChange?.(panel); }, [panel, onPanelChange]);
  /* 续播：拿到时长后再 seek 一次（只对进入时那一条生效） */
  const theater = store.theater;
  const setTheater = store.setTheater;
  const fs = store.fs;
  const setFs = store.setFs;
  const clean = store.clean;
  const setClean = store.setClean;

  /* 追踪手机视口是否已处于物理/系统横屏状态（window.innerWidth > window.innerHeight） */
  const [vpLandscape, setVpLandscape] = useState(
    () => typeof window !== "undefined" && window.innerWidth > window.innerHeight,
  );

  /* 单击清屏 vs 双击暂停计时器 */
  const lastTapRef = useRef<{ time: number; timer: number | null }>({ time: 0, timer: null });
  const clearTapTimer = () => {
    if (lastTapRef.current.timer) {
      window.clearTimeout(lastTapRef.current.timer);
      lastTapRef.current.timer = null;
    }
  };

  /* 桌面：鼠标闲置 2.8s 自动隐藏播放器 UI（沉浸） */
  const [idle, setIdle] = useState(false);
  const idleTimer = useRef<number | undefined>(undefined);
  const wake = useCallback(() => {
    setIdle(false);
    window.clearTimeout(idleTimer.current);
    idleTimer.current = window.setTimeout(() => setIdle(true), 2800);
  }, []);
  useEffect(() => {
    if (!isDesktop) return undefined;
    wake();
    return () => window.clearTimeout(idleTimer.current);
  }, [isDesktop, wake]);
  useEffect(() => {
    if (paused) { setIdle(false); window.clearTimeout(idleTimer.current); }
  }, [paused]);
  const [fullscreen, setFullscreen] = useState(false);

  /* —— 移动端手势 & 竖屏横片缩放 —— */
  const [holdSpeed, setHoldSpeed] = useState(false);
  const [speedLock, setSpeedLock] = useState(false);
  const [lockArmed, setLockArmed] = useState(false);
  const holdRef = useRef(false);
  const lockArmedRef = useRef(false);
  const [scrub, setScrub] = useState<{ pct: number; target: number } | null>(null);
  const scrubTarget = useRef<number | null>(null);
  /* 竖屏下横屏视频适应模式 + 双指无级缩放/平移 */
  const [portraitFit, setPortraitFit] = useState<"contain" | "cover">("contain");
  const [zoom, setZoom] = useState({ scale: 1, x: 0, y: 0, gesturing: false });
  const pinchRef = useRef<{
    dist0: number; midX0: number; midY0: number; scale0: number; x0: number; y0: number;
  } | null>(null);
  /* 进度条拖动 */
  const barScrub = useRef(false);
  const [barPct, setBarPct] = useState<number | null>(null);
  const gesture = useRef<{
    x: number; y: number; startX: number; startY: number; t: number; timer: number | null;
    mode: "idle" | "hold" | "scrub" | "pan"; base: number; width: number; playing: boolean;
  } | null>(null);

  /* 倍速：长按 / 锁定 → 2.0x，否则用基准倍速 */
  const effRate = holdSpeed || speedLock ? 2 : rate;
  /* 进度条显示百分比（拖动时用拖动值） */
  const barProgress = barPct ?? (duration ? time / duration : 0);
  /* 是否为横屏视频（16:9 等宽大于高） */
  const isLandscapeVideo = vsize.w > vsize.h && vsize.h > 0;
  /* 仅当「全屏 + 横屏视频 + 手机系统视口尚未横过来 (!vpLandscape)」时才用 CSS 旋转 90° 兜底；
     一旦系统屏幕已经横过来了 (vpLandscape === true)，不再叠加 CSS 旋转，彻底解决手机横置后视频反而竖过去的问题！ */
  const rotated = !isDesktop && fs && isLandscapeVideo && !vpLandscape;

  /* 借鉴 stclock1988/hghub-ios (PlayerView.swift keepPlayingThroughTransition)：
     iOS 在横竖屏几何旋转或全屏视图层级切换期间，系统底层 AVPlayer / WebKit 视频层可能被短暂挂起暂停。
     在过渡窗口期内分批重申 play()，确保旋转/全屏切换不中断播放。 */
  const keepPlayingTimers = useRef<number[]>([]);
  const keepPlayingThroughTransition = useCallback(() => {
    keepPlayingTimers.current.forEach((id) => window.clearTimeout(id));
    keepPlayingTimers.current = [];
    const el = videoRef.current?.el();
    const shouldPlay = el ? !el.paused : !paused;
    if (!shouldPlay) return;
    for (const delay of [50, 150, 300, 500, 800, 1200]) {
      const tid = window.setTimeout(() => {
        const v = videoRef.current?.el();
        if (v && v.paused && !v.ended) {
          videoRef.current?.play();
        }
      }, delay);
      keepPlayingTimers.current.push(tid);
    }
  }, [paused]);

  useEffect(() => () => {
    keepPlayingTimers.current.forEach((id) => window.clearTimeout(id));
  }, []);

  /* 监听手机屏幕横竖屏变化：物理横置手机时自动进入全屏，竖回时自动退出全屏 */
  useEffect(() => {
    if (isDesktop) return undefined;
    const onViewportChange = () => {
      const isLand = window.innerWidth > window.innerHeight;
      setVpLandscape((prev) => {
        if (prev !== isLand && active) {
          setFs(isLand);
          keepPlayingThroughTransition();
        }
        return isLand;
      });
    };
    window.addEventListener("resize", onViewportChange);
    window.addEventListener("orientationchange", onViewportChange);
    return () => {
      window.removeEventListener("resize", onViewportChange);
      window.removeEventListener("orientationchange", onViewportChange);
    };
  }, [isDesktop, active, setFs, keepPlayingThroughTransition]);

  /* 进入 / 退出全屏：对横屏视频直接调用原生屏幕方向切换，让手机系统界面真正横过来 */
  const enterFs = useCallback(() => {
    if (platform === "ios") {
      const video = videoRef.current?.el(), container = boxRef.current;
      if (video && container) void enterVideoFullscreen(video, container).catch(e => toast(e.message));
      return;
    }
    keepPlayingThroughTransition();
    setClean(false);
    setFs(true);
    if (isLandscapeVideo) {

      try {
        (screen.orientation as unknown as { lock?: (o: string) => Promise<void> })?.lock?.("landscape")?.catch(() => {});
      } catch { /* ignore */ }
    }
  }, [isLandscapeVideo, setClean, setFs, keepPlayingThroughTransition]);

  const exitFs = useCallback(() => {
    keepPlayingThroughTransition();
    setClean(false);
    setFs(false);

    try { screen.orientation?.unlock?.(); } catch { /* ignore */ }
  }, [setClean, setFs, keepPlayingThroughTransition]);

  /* 全屏状态下若视频宽高刚加载出来且为横屏，自动补发一次横屏指令 */
  useEffect(() => {
    if (!isDesktop && fs && isLandscapeVideo) {

    }
  }, [isDesktop, fs, isLandscapeVideo]);


  const go=(delta:number)=>{ if(!active || controlsLocked) return; delta>0 ? onNext?.() : onPrevious?.(); };
  const onTimeTick=(t:number)=>{ if(!barScrub.current && gesture.current?.mode!=="scrub")setTime(t); onPosition(t,videoRef.current?.el()?.duration); };
  const onDurationTick=(d:number)=>setDuration(d);
  useEffect(()=>{setTime(position);setDuration(0);setWaiting(!!media);setErr("");setZoom({scale:1,x:0,y:0,gesturing:false});},[playKey]);
  useEffect(()=>()=>{clearTapTimer();store.setClean(false);store.setFs(false);},[]);
  useEffect(()=>{videoRef.current?.rate(effRate);},[effRate,playKey]);
  useEffect(()=>{if(!panel)return;setBackHandler(()=>{setPanel(null);return true;});return()=>setBackHandler(null);},[panel]);
  useEffect(()=>{const sync=()=>setFullscreen(!!document.fullscreenElement);document.addEventListener("fullscreenchange",sync);return()=>document.removeEventListener("fullscreenchange",sync);},[]);
  const toggleFull=()=>{const el=boxRef.current?.parentElement?.parentElement;if(document.fullscreenElement)void document.exitFullscreen();else void el?.requestFullscreen().catch(()=>toast("当前环境无法进入全屏"));};
  useEffect(()=>{
    if(!active || controlsLocked)return;
    const key=(e:KeyboardEvent)=>{
      if(e.defaultPrevented || document.querySelector("dialog[open]") || (e.target as HTMLElement).closest("input,textarea,select,button,[contenteditable=true]"))return;
      if(panel){if(e.key==="Escape"){e.preventDefault();setPanel(null);}return;}
      switch(e.key.toLowerCase()){
        case "arrowdown":e.preventDefault();go(1);break;case "arrowup":e.preventDefault();go(-1);break;
        case " ":e.preventDefault();videoRef.current?.toggle();break;
        case "arrowleft":e.preventDefault();videoRef.current?.seek(time-5);break;case "arrowright":e.preventDefault();videoRef.current?.seek(time+5);break;
        case "m":setMuted(v=>!v);break;case "t":setTheater(v=>!v);break;case "f":isDesktop?toggleFull():(fs?exitFs():enterFs());break;
        case "c":if(panels.comments)setPanel("comments");break;case "e":if(panels.episodes)setPanel("episodes");break;
      }
      wake();
    };window.addEventListener("keydown",key);return()=>window.removeEventListener("keydown",key);
  },[active,controlsLocked,panel,time,onNext,onPrevious,fs]);
  const panelContent=panel && <><header className="legacy-panel-heading"><nav>{(["detail","episodes","comments","playback"] as const).filter(p=>panels[p]).map(p=><button key={p} aria-pressed={panel===p} onClick={()=>setPanel(p)}>{{detail:"详情",episodes:"选集",comments:"评论",playback:"播放设置"}[p]}</button>)}</nav><button aria-label="关闭面板" onClick={()=>setPanel(null)}><IClose size={16}/></button></header><div className="legacy-panel-body">{panels[panel]}{panel==="playback"&&media?.playback.subtitles?.some(t=>t.format==="vtt")&&<label className="flex flex-col gap-2 mt-4">字幕<select aria-label="字幕" value={subtitle} onChange={e=>{const n=Number(e.target.value);setSubtitle(n);const v=videoRef.current?.el();if(v)Array.from(v.textTracks).forEach((t,i)=>{t.mode=i===n?"showing":"disabled";});}}><option value={-1}>字幕关闭</option>{media.playback.subtitles.filter(t=>t.format==="vtt").map((t,i)=><option key={t.url} value={i}>{t.label||t.language||`字幕 ${i+1}`}</option>)}</select></label>}</div></>;
  /* —— 双指捏合缩放 & 平移（Pinch-to-Zoom & Pan） —— */
  const cancelSingleGesture = () => {
    clearTapTimer();
    const g = gesture.current;
    if (g?.timer) window.clearTimeout(g.timer);
    gesture.current = null;
    if (holdRef.current) {
      holdRef.current = false;
      setHoldSpeed(false);
    }
    lockArmedRef.current = false;
    setLockArmed(false);
    setScrub(null);
    scrubTarget.current = null;
  };

  const onTouchStart = (e: React.TouchEvent<HTMLDivElement>) => {
    if (isDesktop || controlsLocked) return;
    if (e.touches.length === 2) {
      cancelSingleGesture();
      const t0 = e.touches[0];
      const t1 = e.touches[1];
      const dist0 = Math.max(12, Math.hypot(t1.clientX - t0.clientX, t1.clientY - t0.clientY));
      const midX0 = (t0.clientX + t1.clientX) / 2;
      const midY0 = (t0.clientY + t1.clientY) / 2;
      pinchRef.current = {
        dist0,
        midX0,
        midY0,
        scale0: zoom.scale,
        x0: zoom.x,
        y0: zoom.y,
      };
    }
  };

  const onTouchMove = (e: React.TouchEvent<HTMLDivElement>) => {
    if (isDesktop || controlsLocked) return;
    if (e.touches.length === 2 && pinchRef.current) {
      const p = pinchRef.current;
      const t0 = e.touches[0];
      const t1 = e.touches[1];
      const dist = Math.max(12, Math.hypot(t1.clientX - t0.clientX, t1.clientY - t0.clientY));
      const midX = (t0.clientX + t1.clientX) / 2;
      const midY = (t0.clientY + t1.clientY) / 2;
      const nextScale = Math.max(0.85, Math.min(3.5, p.scale0 * (dist / p.dist0)));
      const rawDx = midX - p.midX0;
      const rawDy = midY - p.midY0;
      const dx = rotated ? rawDy : rawDx;
      const dy = rotated ? -rawDx : rawDy;
      const maxX = Math.max(0, (nextScale - 1) * window.innerWidth * 0.55);
      const maxY = Math.max(0, (nextScale - 1) * window.innerHeight * 0.55);
      const nextX = Math.max(-maxX, Math.min(maxX, p.x0 + dx));
      const nextY = Math.max(-maxY, Math.min(maxY, p.y0 + dy));
      setZoom({ scale: nextScale, x: nextX, y: nextY, gesturing: true });
    }
  };

  const onTouchEnd = (e: React.TouchEvent<HTMLDivElement>) => {
    if (isDesktop) return;
    if (pinchRef.current && e.touches.length < 2) {
      pinchRef.current = null;
      setZoom((prev) =>
        prev.scale < 1.08
          ? { scale: 1, x: 0, y: 0, gesturing: false }
          : { ...prev, gesturing: false },
      );
    }
  };

  /* —— 长按 2x（上滑锁定） / 横拖定位 / 单击清屏 / 双击暂停 —— */
  const onDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (isDesktop || controlsLocked || !active || !e.isPrimary || pinchRef.current || gesture.current || e.button !== 0) return;
    const el = e.currentTarget;
    const span = rotated ? (el.clientHeight || window.innerHeight || 1) : (el.clientWidth || 1);
    try { el.setPointerCapture(e.pointerId); } catch { /* ignore */ }
    const g = {
      x: e.clientX, y: e.clientY, startX: e.clientX, startY: e.clientY,
      t: Date.now(), base: time, width: span,
      playing: videoRef.current?.el()?.paused === false,
      mode: "idle" as "idle" | "hold" | "scrub" | "pan",
      timer: window.setTimeout(() => {
        const cur = gesture.current;
        if (!cur || cur.mode !== "idle" || pinchRef.current) return;
        clearTapTimer();
        cur.mode = "hold";
        holdRef.current = true;
        setHoldSpeed(true);
        setLockArmed(false);
        lockArmedRef.current = false;
        try { navigator.vibrate?.(25); } catch { /* ignore */ }
      }, 380) as unknown as number,
    };
    gesture.current = g;
  };

  const onMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (pinchRef.current) {
      cancelSingleGesture();
      return;
    }
    const g = gesture.current;
    if (!g || isDesktop || !e.isPrimary) return;
    const rawDx = e.clientX - g.x;
    const rawDy = e.clientY - g.y;
    const dx = rotated ? rawDy : rawDx;
    const dy = rotated ? -rawDx : rawDy;

    /* 长按中：上滑 64px 进入「松开即锁定」 */
    if (g.mode === "hold") {
      const upDist = rotated ? (e.clientX - g.startX) : (g.startY - e.clientY);
      const armed = upDist > 64;
      if (armed !== lockArmedRef.current) {
        lockArmedRef.current = armed;
        setLockArmed(armed);
      }
      return;
    }

    if (g.mode === "idle") {
      if (Math.abs(dx) > 16 && Math.abs(dx) > Math.abs(dy) * 1.2) {
        clearTapTimer();
        if (g.timer) window.clearTimeout(g.timer);
        g.mode = "scrub";
      } else if (Math.abs(dy) > 16) {
        clearTapTimer();
        if (g.timer) window.clearTimeout(g.timer);
        g.mode = "pan";
      }
      return;
    }

    if (g.mode === "scrub") {
      const total = duration || 0;
      if (!total) return;
      const target = Math.max(0, Math.min(total - 0.2, g.base + (dx / g.width) * total));
      scrubTarget.current = target;
      setScrub({ pct: (target / total) * 100, target });
    }
  };

  const onUp = (e?: React.PointerEvent<HTMLDivElement>, cancelled = false) => {
    if (pinchRef.current) return;
    const g = gesture.current;
    gesture.current = null;
    if (!g) return;
    if (g.timer) window.clearTimeout(g.timer);

    if (g.mode === "hold") {
      holdRef.current = false;
      setHoldSpeed(false);
      if (!cancelled && lockArmedRef.current) {
        setSpeedLock(true);
        toast("已锁定 2.0x 倍速 · 点顶部标签解除", "info");
      }
      lockArmedRef.current = false;
      setLockArmed(false);
      return;
    }
    if (g.mode === "scrub") {
      const target = scrubTarget.current;
      setScrub(null);
      scrubTarget.current = null;
      if (!cancelled && target != null) {
        setErr("");
        setTime(target);
        videoRef.current?.seek(target, "mobile_gesture");
      }
      if (g.playing && active) videoRef.current?.play();
      return;
    }

    if (!cancelled && g.mode === "pan" && e && !fs && !clean && zoom.scale <= 1.05) {
      const dy = e.clientY - g.startY;
      if (Math.abs(dy) > 65) go(dy < 0 ? 1 : -1);
    }
    if (cancelled || g.mode !== "idle") return;
    const moveDist = e ? Math.hypot(e.clientX - g.x, e.clientY - g.y) : 0;
    if (Date.now() - g.t < 260 && moveDist < 16) {
      const now = Date.now();
      if (now - lastTapRef.current.time < 280) {
        // 双击屏幕：切换暂停 / 播放
        clearTapTimer();
        lastTapRef.current.time = 0;
        videoRef.current?.toggle();
      } else {
        // 单击屏幕：切换清屏模式（消除或恢复所有界面控件，只显示视频内容）
        lastTapRef.current.time = now;
        clearTapTimer();
        lastTapRef.current.timer = window.setTimeout(() => {
          lastTapRef.current.timer = null;
          setClean((c) => !c);
        }, 250) as unknown as number;
      }
    }
  };

  useEffect(() => {
    if (!active || controlsLocked) onUp(undefined, true);
    // Cancel pending gestures when the page is hidden or controls are locked.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, controlsLocked]);

  useEffect(() => () => {
    if (gesture.current?.timer) window.clearTimeout(gesture.current.timer);
    gesture.current = null;
  }, [playKey]);

  const lockControls = () => { setPanel(null); setControlsLocked(true); };

  /* 长按倍速 / 双指缩放 / 全屏期间禁止外层列表原生滚动 */
  useEffect(() => {
    if (isDesktop) return undefined;
    const el = boxRef.current;
    if (!el) return undefined;
    const fn = (e: TouchEvent) => {
      if (holdRef.current || pinchRef.current || fs || e.touches.length >= 2) {
        e.preventDefault();
      }
    };
    el.addEventListener("touchmove", fn, { passive: false });
    return () => el.removeEventListener("touchmove", fn);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isDesktop, fs]);

  /* —— 进度条拖动（直接用当前触发元素的 BoundingRect，旋转舞台下按 Y 轴换算） —— */
  const barPctAt = (barEl: HTMLElement | null, clientX: number, clientY: number) => {
    const r = barEl?.getBoundingClientRect();
    if (!r) return null;
    const span = rotated ? r.height : r.width;
    if (!span) return null;
    const raw = rotated ? (clientY - r.top) / span : (clientX - r.left) / span;
    return Math.max(0, Math.min(1, raw));
  };
  const barSeekTo = (barEl: HTMLElement | null, clientX: number, clientY: number) => {
    const pct = barPctAt(barEl, clientX, clientY);
    if (pct == null) return null;
    setBarPct(pct);
    return pct;
  };
  const onBarDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.stopPropagation();
    barScrub.current = true;
    const el = e.currentTarget as HTMLElement;
    try { el.setPointerCapture(e.pointerId); } catch { /* ignore */ }
    barSeekTo(el, e.clientX, e.clientY);
  };
  const onBarMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!barScrub.current) return;
    e.stopPropagation();
    barSeekTo(e.currentTarget as HTMLElement, e.clientX, e.clientY);
  };
  const onBarUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!barScrub.current) return;
    barScrub.current = false;
    e.stopPropagation();
    const pct = barPctAt(e.currentTarget as HTMLElement, e.clientX, e.clientY) ?? barPct;
    setBarPct(null);
    if (pct != null && duration > 0) {
      const target = Math.max(0, Math.min(duration - 0.15, pct * duration));
      setErr("");
      setTime(target);
      videoRef.current?.seek(target, "mobile_bar");
    }
  };

  const togglePip = async () => {
    try { const video = videoRef.current?.el(); if (video) await pictureInPicture(video); }
    catch (e) { toast((e as Error).message || "画中画暂不可用"); }
  };

  const effectiveFit: "contain" | "cover" = isDesktop
    ? "contain"
    : isLandscapeVideo
      ? (fs || vpLandscape ? "contain" : portraitFit)
      : (vpLandscape ? "contain" : "cover");

  return (
    <div
      className={`hg-player-ui absolute inset-0 overflow-hidden bg-black player-root ${controlsLocked ? "is-locked" : ""} ${isDesktop ? "player-surface" : ""} ${theater ? "is-theater" : ""} ${isDesktop && idle && !paused ? "is-idle" : ""}`}
      onPointerMove={() => { if (isDesktop) wake(); }}>
      <div className="absolute inset-0" inert={controlsLocked}>
      {/* 氛围背景（全屏时隐藏，避免透出封面底图） */}
      {!fs && (
        <div className="absolute inset-0 scale-110 blur-3xl opacity-25 pointer-events-none">
          {posterUrl && <img src={posterUrl} alt="" className="w-full h-full object-cover" />}
        </div>
      )}

      <div ref={boxRef} onWheel={e=>{
        if(!isDesktop || controlsLocked || panel || fs || Math.abs(e.deltaY)<35 || Date.now()-lastWheel.current<700 || (e.target as HTMLElement).closest("button,input,select"))return;
        lastWheel.current=Date.now();go(e.deltaY>0?1:-1);
      }}
        className="absolute inset-y-0 left-0 overflow-hidden"
        style={{ right: isDesktop && panel ? 372 : 0, transition: "right .3s cubic-bezier(.2,0,0,1)" }}>
        {[playKey].map((vid) => {
          const isActive = true;
          return (
            <section
              key={vid}
              style={!isActive && fs ? { display: "none" } : undefined}
              className={`relative w-full h-full snap-start snap-always ${fs && isActive ? "" : "overflow-hidden"}`}>
              {isActive && playUrl ? (
                <div className={rotated ? "fs-landscape" : (!isDesktop && fs ? "fs-immersive" : "absolute inset-0")}>
                  <div
                    className="absolute inset-0 overflow-hidden"
                    style={{ touchAction: "none" }}
                    onTouchStart={onTouchStart}
                    onTouchMove={onTouchMove}
                    onTouchEnd={onTouchEnd}
                    onTouchCancel={onTouchEnd}
                    onPointerDown={onDown}
                    onPointerMove={onMove}
                    onPointerUp={(e) => onUp(e, false)}
                    onPointerCancel={(e) => onUp(e, true)}
                    onLostPointerCapture={(e) => onUp(e, true)}>
                    <VideoSurface
                      ref={videoRef}
                      media={media!} position={position}
                      poster={fs ? undefined : posterUrl}
                      autoPlay
                      active={active}
                      
                      muted={muted}
                      volume={volume}
                      rate={effRate}
                      fit={effectiveFit}
                      scale={zoom.scale}
                      panX={zoom.x}
                      panY={zoom.y}
                      isGesturing={zoom.gesturing}
                      onClick={() => { if (isDesktop) videoRef.current?.toggle(); }}
                      onTime={onTimeTick}
                      onDuration={onDurationTick}
                      onSize={(w, h) => setVsize({ w, h })}
                      onPlaying={() => { setPaused(false); setWaiting(false); setErr(""); onPlaying?.(); }}
                      onPause={() => { setPaused(true); onStopped?.(); }}
                      onWaiting={() => setWaiting(true)}
                      onEnded={onEnded} onBuffered={setBuffered}
                      onError={() => { setErr("播放失败"); onFailure(); }}
                    />
                  </div>

                  {/* 横拖定位浮层 */}
                  {!isDesktop && scrub && (
                    <div className="absolute inset-0 z-30 flex flex-col items-center justify-center pointer-events-none" style={{ background: "rgba(0,0,0,.25)" }}>
                      <span className="text-[26px] font-bold hg-ink tabular-nums hg-ov-shadow">{fmtTime(scrub.target)}</span>
                      <span className="text-[11px] hg-ink-4 mt-1 tabular-nums">/ {fmtTime(duration)}</span>
                      <div className="mt-4 w-[62%] h-[3px] rounded-full overflow-hidden" style={{ background: "rgba(255,255,255,.25)" }}>
                        <div className="h-full bg-white" style={{ width: `${scrub.pct}%` }} />
                      </div>
                    </div>
                  )}

                  {/* 暂停 / 缓冲 / 报错恢复 */}
                  {paused && !err && (
                    <div className="absolute inset-0 z-10 flex items-center justify-center pointer-events-none">
                      <span className="w-[66px] h-[66px] rounded-full flex items-center justify-center backdrop-blur-md"
                        style={{ background: "rgba(0,0,0,.55)", boxShadow: "inset 0 0 0 1px rgba(255,255,255,.12)" }}>
                        <IPlayF size={28} className="hg-ink ml-0.5" />
                      </span>
                    </div>
                  )}
                  {waiting && !err && !paused && (
                    <div className="absolute inset-0 z-10 flex items-center justify-center pointer-events-none"><div className="w-9 h-9 spinner" /></div>
                  )}
                  {err && (
                    <div className="absolute inset-0 z-30 flex items-center justify-center pointer-events-none">
                      <div className="rounded-xl p-5 flex flex-col items-center gap-3 text-[13px] hg-ink-2 pointer-events-auto" style={{ background: "rgba(0,0,0,.74)" }}>
                        <span>视频暂时无法加载</span>
                        <span className="text-[10.5px] hg-ink-4 max-w-[240px] text-center break-all">{error || err}</span>
                        <button className="px-4 py-1.5 rounded-full hg-chip-strong hg-link text-[12.5px] font-semibold pressable"
                          onClick={() => { setErr(""); onRetry?.(); }}>
                          重新加载
                        </button>
                      </div>
                    </div>
                  )}

                  {/* 移动端：倍速状态 / 锁定提示 */}
                  {!isDesktop && (!clean || holdSpeed) && (holdSpeed || speedLock || rate !== 1) && (
                    <button
                      onClick={() => {
                        if (speedLock) { setSpeedLock(false); toast("已解除倍速锁定", "info"); }
                        else if (rate !== 1 && !holdSpeed) setRate(1);
                      }}
                      className={`absolute left-1/2 -translate-x-1/2 z-30 flex items-center gap-1.5 px-3.5 h-8 rounded-full backdrop-blur-md text-[12px] font-bold elev-2 pressable ${speedLock ? "bg-primary text-on-primary" : "hg-chip hg-gold"}`}
                      style={{ top: rotated || vpLandscape ? 14 : "calc(var(--sat, 0px) + 54px)" }}>
                      <IBoltF size={13} />
                      {effRate}x {speedLock ? "已锁定 · 点按解除" : holdSpeed ? "倍速中 · 松开恢复" : "倍速"}
                    </button>
                  )}
                  {!isDesktop && holdSpeed && (
                    <div
                      className={`absolute left-1/2 -translate-x-1/2 z-30 flex items-center gap-2 px-4 h-10 rounded-xl backdrop-blur-md elev-2 transition-all duration-200 ${lockArmed ? "bg-primary text-on-primary scale-110" : "hg-chip-strong hg-ink-2"}`}
                      style={{ top: "38%" }}>
                      <ILockF size={16} />
                      <span className="text-[12.5px] font-bold">{lockArmed ? "松开锁定 2.0x" : "上滑到此锁定倍速"}</span>
                    </div>
                  )}

                  {/* 移动端：沉浸全屏控制栏（清屏 clean 模式下隐藏，单击屏幕可呼出/隐藏） */}
                  {!isDesktop && fs && !clean && !controlsLocked && (
                    <>
                      <div className="absolute right-4 z-40 flex items-center gap-2"
                        style={{ top: rotated || vpLandscape ? 14 : "calc(var(--sat, 0px) + 8px)" }}>
                        <button onClick={lockControls} aria-label="锁定播放控制" title="锁定播放控制" aria-pressed={false}
                          className="hg-chip-strong w-9 h-9 rounded-full backdrop-blur-md flex items-center justify-center pressable elev-1">
                          <ILockF size={17} />
                        </button>
                        <button onClick={exitFs} aria-label="退出全屏"
                          className="hg-chip-strong w-9 h-9 rounded-full backdrop-blur-md flex items-center justify-center pressable elev-1">
                          <ICompress size={18} />
                        </button>
                      </div>
                      {zoom.scale > 1.05 && (
                        <button
                          onClick={() => setZoom({ scale: 1, x: 0, y: 0, gesturing: false })}
                          className="hg-chip-strong absolute left-4 z-40 px-3 h-8 rounded-full backdrop-blur-md text-[11.5px] font-semibold flex items-center gap-1 pressable elev-1"
                          style={{ top: rotated || vpLandscape ? 14 : "calc(var(--sat, 0px) + 8px)" }}>
                          还原画面 ({zoom.scale.toFixed(1)}x)
                        </button>
                      )}
                      <div className="absolute inset-x-0 z-30 px-4" style={{ bottom: rotated || vpLandscape ? 46 : "calc(var(--sab, 0px) + 52px)", height: 26 }}>
                        <div className="relative h-full" style={{ touchAction: "none" }}
                          onPointerDown={onBarDown} onPointerMove={onBarMove} onPointerUp={onBarUp} onPointerCancel={onBarUp}>
                          <div className={`absolute inset-x-0 bottom-[10px] rounded-full transition-all ${barPct !== null ? "h-[5px]" : "h-[3px]"}`}
                            style={{ background: "rgba(255,255,255,.3)" }}>
                            <div className="h-full rounded-full bg-white" style={{ width: `${barProgress * 100}%` }} />
                          </div>
                          {barPct !== null && (
                            <div className="absolute left-1/2 -translate-x-1/2 -top-8 px-2.5 py-1 rounded-md hg-chip-strong backdrop-blur-md text-[12px] font-bold tabular-nums whitespace-nowrap">
                              {fmtTime(barPct * (duration || 0))} / {fmtTime(duration)}
                            </div>
                          )}
                        </div>
                      </div>
                      <div className="absolute inset-x-0 z-40 flex items-center gap-3 px-4"
                        style={{ bottom: rotated || vpLandscape ? 10 : "calc(var(--sab, 0px) + 10px)" }}>
                        <button onClick={() => videoRef.current?.toggle()}
                          className="hg-chip shrink-0 w-9 h-9 rounded-full flex items-center justify-center pressable">
                          {paused ? <IPlayF size={18} /> : <IPauseF size={17} />}
                        </button>
                        <span className="shrink-0 text-[11.5px] font-semibold hg-ink-2 tabular-nums">{fmtTime(time)}</span>
                        <button
                          onClick={() => {
                            if (speedLock) { setSpeedLock(false); return; }
                            const seq = [1, 1.25, 1.5, 2];
                            const idx = seq.indexOf(rate);
                            setRate(seq[(idx + 1) % seq.length]);
                          }}
                          className={`ml-auto shrink-0 flex items-center gap-1 px-2.5 h-8 rounded-full text-[12px] font-bold pressable ${effRate !== 1 ? "bg-primary text-on-primary" : "hg-chip hg-ink-2"}`}>
                          <IBoltF size={12} />{effRate}x
                        </button>
                        <span className="shrink-0 text-[11.5px] hg-ink-4 tabular-nums">{fmtTime(duration)}</span>
                      </div>
                    </>
                  )}
                </div>
              ) : (
                <div className="absolute inset-0">
                  {posterUrl && <img src={posterUrl} alt="" className="w-full h-full object-contain"/>}{!error && <div className="absolute inset-0 flex items-center justify-center text-white/60 text-xs">正在准备播放…</div>}
                  <div className="absolute inset-0" style={{ background: "rgba(0,0,0,.25)" }} />
                </div>
              )}

              {/* 取不到播放地址时播放器不会创建，错误提示要放在播放器外面，否则只会停在封面 */}
              {isActive && !playUrl && !!error && (
                <div className="absolute inset-0 z-30 flex items-center justify-center pointer-events-none">
                  <div className="rounded-xl p-5 flex flex-col items-center gap-3 text-[13px] hg-ink-2 pointer-events-auto" style={{ background: "rgba(0,0,0,.74)" }}>
                    <span>视频暂时无法加载</span>
                    <span className="text-[10.5px] hg-ink-4 max-w-[240px] text-center break-all">{error || err}</span>
                    <button className="px-4 py-1.5 rounded-full hg-chip-strong hg-link text-[12.5px] font-semibold pressable"
                      onClick={() => { setErr(""); setWaiting(true); onRetry?.(); }}>
                      重新加载
                    </button>
                  </div>
                </div>
              )}

              {/* 渐变压暗，保证文字可读（清屏模式下不加压暗遮罩，只显示纯视频内容） */}
              {!fs && !clean && (
                <div className="absolute inset-0 pointer-events-none"
                  style={{ background: "linear-gradient(0deg, rgba(8,11,15,.86) 0, rgba(11,14,20,.18) 26%, transparent 48%)" }} />
              )}

              {/* 竖屏下观看横屏视频：快捷「放大铺满 / 完整显示 / 全屏横屏」与双指缩放复位胶囊 */}
              {isActive && !fs && !clean && !isDesktop && (isLandscapeVideo || zoom.scale > 1.05) && (
                <div
                  className="absolute left-1/2 -translate-x-1/2 z-20 flex items-center gap-2"
                  style={{ top: "64%" }}>
                  {isLandscapeVideo && (
                    <>
                      <button
                        type="button"
                        onClick={() => {
                          if (zoom.scale > 1.05) {
                            setZoom({ scale: 1, x: 0, y: 0, gesturing: false });
                            setPortraitFit("contain");
                          } else {
                            setPortraitFit((f) => (f === "contain" ? "cover" : "contain"));
                          }
                        }}
                        className="hg-chip-strong backdrop-blur-md px-3 h-7 rounded-full text-[11.5px] font-semibold flex items-center gap-1.5 pressable elev-1">
                        {zoom.scale > 1.05
                          ? `还原缩放 (${zoom.scale.toFixed(1)}x)`
                          : portraitFit === "contain"
                            ? "放大铺满"
                            : "完整适应"}
                      </button>
                      <button
                        type="button"
                        onClick={enterFs}
                        className="hg-chip-strong backdrop-blur-md px-3 h-7 rounded-full text-[11.5px] font-semibold flex items-center gap-1 pressable elev-1">
                        <IExpand size={12} />横屏全屏
                      </button>
                    </>
                  )}
                  {!isLandscapeVideo && zoom.scale > 1.05 && (
                    <button
                      type="button"
                      onClick={() => setZoom({ scale: 1, x: 0, y: 0, gesturing: false })}
                      className="hg-chip-strong backdrop-blur-md px-3 h-7 rounded-full text-[11.5px] font-semibold flex items-center gap-1 pressable elev-1">
                      还原画面 ({zoom.scale.toFixed(1)}x)
                    </button>
                  )}
                </div>
              )}

              {!fs && !clean && onBack && <div className={isDesktop ? "desktop-context-header" : "absolute left-3 z-20"} style={isDesktop ? undefined : {top:"calc(var(--sat, 0px) + 6px)"}}><button onClick={onBack} aria-label="返回" className="hg-chip w-9 h-9 rounded-full flex items-center justify-center"><IBack size={20}/></button></div>}
              {/* 右侧操作栏（移动端）：收藏 / 评论 / 全屏 / 分享
                  颜色写死不跟主题走：浅色主题下这些字必须仍是白色 */}
              {!fs && !clean && !isDesktop && (
                <div className="absolute right-3 z-20 flex flex-col items-center gap-[16px]"
                  style={{ bottom: `calc(var(--sab, 0px) + ${bottomInset + 92}px)` }}>
                  {onFavorite && <button onClick={onFavorite} className="flex flex-col items-center gap-1 pressable" aria-pressed={favorite} aria-label={favorite ? "取消本地收藏" : "本地收藏"}>
                    {favorite ? <IStarF size={27} className="hg-gold" /> : <IStar size={27} className="hg-ink hg-ov-shadow" />}
                    <span className="text-[10.5px] hg-ink-2 hg-ov-shadow">收藏</span>
                  </button>}
                  {panels.comments && <button onClick={() => setPanel("comments")} className="flex flex-col items-center gap-1 pressable" aria-label="评论">
                    <ICommentF size={26} className="hg-ink hg-ov-shadow" />
                    <span className="text-[10.5px] hg-ink-2 hg-ov-shadow">评论</span>
                  </button>}
                  {platform === "ios" && <button onClick={togglePip} className="flex flex-col items-center gap-1 pressable" aria-label="画中画"><IPip size={24}/><span className="text-[10.5px] hg-ink-2">画中画</span></button>}
                  <button onClick={enterFs} className="flex flex-col items-center gap-1 pressable" aria-label="全屏">
                    <IExpand size={26} className="hg-ink hg-ov-shadow" />
                    <span className="text-[10.5px] hg-ink-2 hg-ov-shadow">全屏</span>
                  </button>
                </div>
              )}

              {/* 右侧操作栏（桌面） */}
              {!fs && isDesktop && (
              <div className="absolute z-20 flex flex-col items-center desktop-social">
                <div className="desktop-browse-inline">
                  <button onClick={() => go(-1)} disabled={!onPrevious} aria-label="上一个" title="上一个 ↑">
                    <IChevronUp size={19} strokeWidth={1.8} />
                  </button>
                  <button onClick={() => go(1)} disabled={!onNext} aria-label="下一个" title="下一个 ↓">
                    <IChevronDown size={19} strokeWidth={1.8} />
                  </button>
                  <small>{String(queueIndex + 1).padStart(2, "0")} / {String(total).padStart(2, "0")}</small>
                </div>

                {(onFavorite || panels.episodes) && (
                  <>
                    {social}
                    {panels.comments && <button onClick={() => setPanel("comments")} className="flex flex-col items-center gap-1 pressable">
                      <ICommentF size={27} className="text-white tsh" />
                      <span className="text-[10.5px] text-white/90 tabular-nums">{meta.comment ?? "评论"}</span>
                    </button>}

                    {onFavorite && <button onClick={onFavorite} className="flex flex-col items-center gap-1 pressable" aria-pressed={favorite} aria-label={favorite ? "取消本地收藏" : "本地收藏"}>
                      {favorite ? <IStarF size={27} className="hg-gold" /> : <IStar size={27} className="hg-ink hg-ov-shadow" />}
                      <span className="text-[10.5px] text-white/90 tabular-nums">{"收藏"}</span>
                    </button>}

                    {panels.episodes && <button onClick={() => setPanel("episodes")} className="flex flex-col items-center gap-1 pressable">
                      <IList size={26} className="text-white tsh" />
                      <span className="text-[10.5px] text-white/90">选集</span>
                    </button>}
                  </>
                )}
              </div>
              )}

              {!fs && !clean && panels.playback && <button className="legacy-playback-settings" aria-label="播放设置" onClick={() => setPanel("playback")}><ISettings size={18}/></button>}
              {/* 桌面：左下信息 */}
              {!fs && isDesktop && (
              <div className="desktop-video-meta">
                <div className="flex items-center gap-2">
                  <span className="text-[15px] font-bold text-white tsh truncate">@{meta.author}</span>
                  {episodeCount > 0 && (
                    <span className="shrink-0 px-1.5 py-px rounded-md text-[10px] font-bold text-white"
                      style={{ background: "rgba(255,255,255,.16)" }}>
                      {curEp} / {episodeCount} 集
                    </span>
                  )}
                  {!!meta.score && (
                    <span className="shrink-0 text-[11px] font-bold hg-gold hg-ov-shadow">{meta.score.toFixed(1)} 分</span>
                  )}
                </div>

                <p className="mt-1 text-[13.5px] leading-snug hg-ink-2 hg-ov-shadow clamp-2">{meta.title}</p>

                {/* 描述（单行，点开展开） */}
                <button onClick={() => setPanel("detail")} className="mt-1 text-left w-full pressable group">
                  <p className={`text-[11.5px] leading-relaxed hg-ink-4 clamp-1`}>
                    {meta.desc || "暂无简介"}
                  </p>
                  <span className="text-[10.5px] hg-link font-semibold">查看详情</span>
                </button>
              </div>
              )}

              {/* 移动端：标题 + 详情标签 + 进度线（清屏 clean 模式下隐藏） */}
              {!fs && !clean && !isDesktop && (
                <div className="absolute inset-x-0 z-20" style={{ bottom: `calc(var(--sab, 0px) + ${bottomInset}px)` }}>
                  <div className="absolute inset-x-0 bottom-0 h-[190px] pointer-events-none"
                    style={{ background: "linear-gradient(0deg, rgba(0,0,0,.82) 0%, rgba(0,0,0,.42) 45%, transparent 100%)" }} />
                  <p className="relative px-4 pb-2 text-[14px] font-semibold leading-snug hg-ink hg-ov-shadow clamp-2">{meta.title}</p>
                  <div className="relative pt-1.5 pb-2">
                    <button onClick={() => setPanel("detail")}
                      className="px-4 flex items-center gap-0.5 text-[10.5px] font-medium hg-ink hg-ov-shadow pressable">
                      详情 · {curEp}<IChevR size={11} />
                    </button>
                    {isActive && (
                      <div className="mx-4 mt-1.5 relative" style={{ height: 22, touchAction: "none" }}
                        onPointerDown={onBarDown} onPointerMove={onBarMove} onPointerUp={onBarUp} onPointerCancel={onBarUp}>
                        <div className={`absolute inset-x-0 top-1/2 -translate-y-1/2 rounded-full transition-all ${barPct !== null ? "h-[5px]" : "h-[2.5px]"}`}
                          style={{ background: "rgba(255,255,255,.3)" }}>
                          <div className="h-full rounded-full bg-white"
                            style={{ width: `${barProgress * 100}%`, transition: barPct !== null ? "none" : "width .25s linear" }} />
                        </div>
                        {barPct !== null && (
                          <div className="absolute left-1/2 -translate-x-1/2 -top-8 px-2.5 py-1 rounded-md hg-chip-strong backdrop-blur-md text-[12px] font-bold tabular-nums whitespace-nowrap">
                            {fmtTime(barPct * (duration || 0))} / {fmtTime(duration)}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              )}
            </section>
          );
        })}
      </div>

      {/* 桌面控制条 */}
      {isDesktop && (
        <PlayerControls
          paused={paused} time={time} duration={duration} buffered={buffered} rate={rate}
          muted={muted} volume={volume} theater={theater} fullscreen={fullscreen}
          visible={active && (!idle || paused)}
          seekEnabled={active && !controlsLocked}
          seekKey={playKey}
          nextDisabled={!onNextEpisode && !onNext}
          nextTitle={onNextEpisode ? "下一集" : "下一部"}
          onToggle={() => videoRef.current?.toggle()}
          onSeek={(t, source) => { videoRef.current?.seek(t, source); setTime(t); }}
          onRate={(r) => { setRate(r); videoRef.current?.rate(r); }}
          onMute={() => setMuted((m) => !m)}
          onVolume={(v) => setVolume(v)}
          onPip={togglePip}
          onLock={lockControls}
          onTheater={() => setTheater((v) => !v)}
          onFullscreen={toggleFull}
          onNext={onNextEpisode || onNext}
        />
      )}

      {isDesktop ? <div className={`desktop-dock-panel ${panel ? "is-open" : ""}`}><div className="desktop-dock-inner">{panelContent}</div></div> : <Sheet open={!!panel} onClose={() => setPanel(null)}><div className="flex flex-col" style={{height:"72vh"}}>{panelContent}</div></Sheet>}
      </div>
      {hint&&<div role="status" className="absolute top-1/3 left-1/2 -translate-x-1/2 z-50 px-4 py-2 rounded-full bg-black/70 text-white text-xs pointer-events-none">{hint}</div>}
      {!isDesktop && !fs && !clean && !controlsLocked && active && (
        <div className="absolute right-3 z-40 flex items-center gap-2" style={{ top: "calc(var(--sat, 0px) + 6px)" }}>
          <button onClick={() => setMuted(m => !m)} aria-label={muted ? "取消静音" : "静音"}
            className="hg-chip w-9 h-9 rounded-full flex items-center justify-center pressable elev-1">
            {muted ? <IVolMute size={17} /> : <IVol size={17} />}
          </button>
          <button onClick={lockControls} aria-label="锁定播放控制" title="锁定播放控制" aria-pressed={false}
            className="hg-chip w-9 h-9 rounded-full flex items-center justify-center pressable elev-1"><ILockF size={17} /></button>
          <button onClick={() => store.setTab("search")} aria-label="搜索"
            className="hg-chip w-9 h-9 rounded-full flex items-center justify-center pressable elev-1"><ISearch size={18} /></button>
        </div>
      )}
      {controlsLocked && <PlaybackLock active={active} onUnlock={() => { setControlsLocked(false); wake(); }} />}
    </div>
  );
}
