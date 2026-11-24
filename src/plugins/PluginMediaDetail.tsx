import { useEffect, useMemo, useRef, useState } from "react";
import { useHost, libraryKey, type LocalProgress } from "./HostContext";
import { pluginCommand, pluginError, releaseMedia, onPluginAuthSuccess, onPluginAuthFailure, type InstalledPlugin, type PreparedMedia, type PlaybackChoices } from "./client";
import { NextEpisode } from './nextEpisode';
import { playbackTiming } from './playbackTiming';
import { initialPlaybackSelection, rememberPlaybackSelection, type PlaybackSelection } from "./playbackSelection";
import { usePluginRequests } from "./usePluginRequests";
import PluginPlayer from "./PluginPlayer";
import type { FeedPresentation } from "../components/Feed";
import { useAssetUrls } from "./MediaCards";
import type { Episode, MediaDetail, MediaItem, Page } from "./protocol";
import { Notice } from "./BrowsePages";
import CommentsPanel from "./CommentsPanel";
import MediaInteractions from "./MediaInteractions";
import { pluginProgressReporter } from "./progressReporter";
import SourceSwitcher from "./SourceSwitcher";
import type { SourceStart } from "./sourceMatching";
import ResolverChooser from "./ResolverChooser";
import ExternalPlayback from './ExternalPlayback';
import { useStore } from "../store";
import { registerBackHandler } from "../back";
import { setMobileOrientation } from "../orientation";
import { isMobile } from "../environment";
import MediaDetailSheet from "./MediaDetailSheet";
import { peekMetadata, storeMetadata } from './metadataCache';

/** Only explicit season/episode identities can suggest alignment. Never infer from index. */
export function alignedEpisode(previous: Episode, episodes: Episode[]) {
  if (previous.episodeNumber === undefined) return undefined;
  const matches = episodes.filter(e => e.episodeNumber === previous.episodeNumber && e.season === previous.season);
  return matches.length === 1 ? matches[0] : undefined;
}
export default function PluginMediaDetail({ plugin, mediaId, onBack, manage, active = true, start, onSwitch, presentation = {}, entryEpisodeId, entryItem }: {
  plugin: InstalledPlugin; mediaId: string; onBack?: () => void; manage: () => void; active?: boolean; presentation?: FeedPresentation; entryEpisodeId?: string; entryItem?: MediaItem;
  start?: SourceStart; onSwitch: (pluginId: string, mediaId: string, start?: SourceStart) => void;
}) {
  const app = useStore();
  const viewActive = useRef(active); viewActive.current = active;
  const host = useHost(), request = usePluginRequests(), metadataRequest = usePluginRequests();
  const [detail, setDetail] = useState<MediaDetail | null>(() => peekMetadata<MediaDetail>(plugin, 'media.getDetail', { id: mediaId }) || null), [error, setError] = useState("");
  const homePreview = presentation.mode === "preview";
  const [playIntent, setPlayIntent] = useState(0);
  const [fullPlayback, setFullPlayback] = useState(!!start);
  const [detailOpen, setDetailOpen] = useState(!homePreview && !start);
  const [detailRound, setDetailRound] = useState(0);
  const [needsDetail, setNeedsDetail] = useState(!entryEpisodeId || !homePreview);
  const [episodePage, setEpisodePage] = useState(0);
  const playbackRequested = homePreview || fullPlayback;
  const leavePlayback = () => {
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => {});
    if (isMobile) void setMobileOrientation(false).catch(() => {});
    setDetailOpen(false); setFullPlayback(false); app.setFs(false); app.setClean(false); app.setTheater(false);
    if (!homePreview) onBack?.();
  };
  const leaveRef = useRef(leavePlayback); leaveRef.current = leavePlayback;
  useEffect(() => { if (!fullPlayback || !active) return; app.setWatching(true); return () => app.setWatching(false); }, [fullPlayback, active]);
  useEffect(() => { if (!fullPlayback || !active) return; return registerBackHandler(() => {
    if (document.querySelector(".desktop-dock-panel.is-open")) return false;
    leaveRef.current(); return true;
  }); }, [fullPlayback, active]);
  const openDetail = () => { setNeedsDetail(true); setDetailOpen(true); };
  const [playlist, setPlaylist] = useState(""), [episode, setEpisode] = useState<Episode | null>(null);
  const [media, setMedia] = useState<PreparedMedia | null>(null), [busy, setBusy] = useState(false);
  const [choices, setChoices] = useState<PreparedMedia["choices"] | null>(null);
  const [switching, setSwitching] = useState(false), startedFromSource = useRef(false);
  const selection = useRef(initialPlaybackSelection(plugin.meta.id));
  const attempted = useRef(new Set<string>());
  const sequence = useRef(0), position = useRef(0), refreshed = useRef(false), current = useRef<{ ep: Episode; playlistId?: string } | null>(null);
  const detailRef = useRef<MediaItem | null>(detail || entryItem || null), latestHost = useRef(host); detailRef.current = detail || entryItem || null; latestHost.current = host;
  const session = useRef<PreparedMedia | null>(null), lastSave = useRef(0);
  const failedWhileHidden = useRef(false);
  const transitioning = useRef(false), bufferAhead = useRef(0), playing = useRef(false);
  const durationRef = useRef(0), pagePending = useRef('');
  const prefetch = useMemo(() => new NextEpisode({
    prepare: (params, requestId) => pluginCommand<PreparedMedia>('media.prepare', { ...params, requestId }, { notifyAuth: false }),
    cancel: requestId => { void pluginCommand('cancel', { requestId }).catch(() => {}); },
    release: releaseMedia,
    warm: async (media, params, signal) => {
      if (!viewActive.current || !playing.current || document.hidden || signal.aborted) return false;
      const args = params.args as { playlistId?: string; episodeId: string };
      const saved = await pluginCommand<LocalProgress | null>('library.progress.get', { pluginId: plugin.meta.id, mediaId, playlistId: args.playlistId, episodeId: args.episodeId });
      if (!viewActive.current || !playing.current || document.hidden || signal.aborted) return false;
      const result = await pluginCommand<{ bytes: number }>('media.preload', { sessionId: media.sessionId, positionSec: saved?.positionSec || 0 });
      return result.bytes > 0;
    },
    stopWarm: sessionId => { void pluginCommand('media.preload.cancel', { sessionId }).catch(() => {}); },
    activate: sessionId => pluginCommand('media.activity', { sessionId, playing: true }),
  }), [plugin.meta.id, plugin.revision, mediaId]);
  const environmentKey = JSON.stringify(host.plugins.map(p => [p.meta.id, p.revision, p.enabled, p.endpoint?.url]));
  const preparationKey = (ep: Episode, listId: string | undefined, chosen: PlaybackSelection) => JSON.stringify([environmentKey, mediaId, listId, ep.id, chosen]);
  useEffect(() => {
    const clear = () => prefetch.clear();
    const hidden = () => { if (document.hidden) clear(); };
    const success = onPluginAuthSuccess(clear), failure = onPluginAuthFailure(clear);
    window.addEventListener('hpp.changed', clear); document.addEventListener('visibilitychange', hidden);
    return () => { clear(); success(); failure(); window.removeEventListener('hpp.changed', clear); document.removeEventListener('visibilitychange', hidden); };
  }, [prefetch, environmentKey]);
  useEffect(() => { if (!active || !fullPlayback || detailOpen) prefetch.clear(); }, [active, fullPlayback, detailOpen, prefetch]);
  const entry = host.library.find(e => libraryKey(e.pluginId, e.mediaId) === libraryKey(plugin.meta.id, mediaId));
  const canPlay = plugin.enabled && ["ready", "unloaded"].includes(plugin.status);
  const progress = useMemo(() => pluginProgressReporter(plugin.meta.id, mediaId, plugin.methods.includes("progress.report")), [plugin.meta.id, mediaId, plugin.revision]);
  useEffect(() => () => progress.pause(), [progress]);
  const save = async () => {
    const c = current.current, d = detailRef.current, seconds = position.current;
    if (!c || !d) return;
    await pluginCommand("library.put", { pluginId: plugin.meta.id, item: d });
    await pluginCommand("library.progress.set", { pluginId: plugin.meta.id, mediaId, playlistId: c.playlistId, episodeId: c.ep.id, positionSec: seconds, durationSec: c.ep.durationSec });
    await latestHost.current.refreshLibrary();
  };
  const saveRef = useRef(save); saveRef.current = save;
  useEffect(() => { if (detail) storeMetadata(plugin, 'media.getDetail', { id: mediaId }, detail); }, [detail]);
  useEffect(() => {
    if (!needsDetail) return;
    let live = true;
    void metadataRequest.read<MediaDetail>(plugin, 'media.getDetail', { id: mediaId }, detailRound > 0).then(d => {
      if (!live) return;
      setDetail(d);
      const liveList = d.playlists.find(p => p.episodes.some(ep => ep.id === current.current?.ep.id));
      const chosenList = liveList || d.playlists.find(p => p.id === entry?.lastProgress?.playlistId) || d.playlists[0];
      setPlaylist(chosenList?.id || "");
      if (liveList && current.current) {
        const liveEpisode = liveList.episodes.find(ep => ep.id === current.current!.ep.id)!;
        current.current = { ep: liveEpisode, playlistId: liveList.id }; setEpisode(liveEpisode);
      }
      if (entry?.tracking) void host.put(plugin.meta.id, d).catch(e => setError(pluginError(e)));
    }).catch(e => { if (live && e?.code !== 'CANCELLED') setError(pluginError(e)); });
    return () => { live = false; };
  }, [plugin.meta.id, plugin.revision, mediaId, metadataRequest, needsDetail, detailRound]);
  useEffect(() => () => { autoStarted.current=false; startedFromSource.current=false; sequence.current++; if (session.current) releaseMedia(session.current.sessionId); void saveRef.current().catch(() => {}); }, [plugin.meta.id, mediaId]);
  useEffect(() => {
    if (!active || !canPlay) {
      if (!session.current || !canPlay) { autoStarted.current = false; startedFromSource.current = false; }
      progress.pause();
      sequence.current++; request.cancelAll();
      transitioning.current = false;
      setBusy(false);
      if (!canPlay) { if (session.current) releaseMedia(session.current.sessionId); session.current = null; setMedia(null); }
      void saveRef.current().catch(e => setError(pluginError(e)));
    } else if (current.current && playbackRequested && (failedWhileHidden.current || !session.current || (session.current.playback.expiresAt && session.current.playback.expiresAt <= Date.now() + 5000))) {
      autoStarted.current = true;
      failedWhileHidden.current = false;
      void play(current.current.ep, 'refresh', current.current.playlistId);
    }
  }, [active, canPlay]);
  const play = async (ep: Episode, reason: "play" | "refresh" | "switch" | "fallback" = "play", listId: string | undefined = playlist || undefined, chosen = selection.current, proposedPosition?: number) => {
    if (!viewActive.current || !canPlay) return;
    const n = ++sequence.current;
    const started = performance.now();
    setBusy(true); setError(""); setChoices(null); transitioning.current = reason !== 'refresh';
    if (reason !== 'refresh') { playing.current = false; progress.pause(); }
    if (reason !== "refresh") { void save().catch(() => {}); request.cancelAll(); }
    if (n !== sequence.current) return;
    setEpisode(ep);
    const previousSession = session.current;
    try {
      if (reason === "play") {
        setChoices(null);
        chosen = { ...chosen, streamIndex: 0, resolverStreamIndex: 0 };
      }
      if (reason === "switch") refreshed.current = false;
      if (reason === "play" || reason === "switch") attempted.current.clear();
      selection.current = chosen;
      const { serverId, ...options } = chosen;
      const prepared = async () => {
        if (reason === 'play') {
          const cached = await prefetch.take(preparationKey(ep, listId, chosen));
          if (cached) { playbackTiming(cached.sessionId, 'prepared-hit', started); return cached; }
        } else prefetch.clear();
        if (n !== sequence.current) throw { code: 'CANCELLED' };
        if (reason === 'refresh' && previousSession) return request<PreparedMedia>('media.refresh', { sessionId: previousSession.sessionId, version: previousSession.version });
        return request<PreparedMedia>("media.prepare", { id: plugin.meta.id, ...options, args: { mediaId, episodeId: ep.id, playlistId: listId, serverId, reason: reason === "refresh" ? "refresh" : "play" } });
      };
      const [preparedResult, savedResult] = await Promise.allSettled([
        prepared(),
        reason === 'play' ? request<LocalProgress | null>('library.progress.get', { pluginId: plugin.meta.id, mediaId, playlistId: listId, episodeId: ep.id }) : Promise.resolve(null),
      ]);
      if (preparedResult.status === 'rejected') throw preparedResult.reason;
      const r = preparedResult.value;
      if (savedResult.status === 'rejected') { releaseMedia(r.sessionId); throw savedResult.reason; }
      if (n !== sequence.current) { if (r.sessionId !== previousSession?.sessionId) releaseMedia(r.sessionId); return; }
      if (reason === 'play') {
        position.current = savedResult.value?.positionSec ?? proposedPosition ?? 0; refreshed.current = false;
        progress.select(ep.id, position.current);
        current.current = { ep, playlistId: listId };
      }
      selection.current = rememberPlaybackSelection(r, chosen);
      r.requestedAt = started;
      session.current = r; setMedia(r); setChoices(r.choices);
      bufferAhead.current = 0; durationRef.current = 0; playing.current = false;
      if (previousSession && previousSession.sessionId !== r.sessionId) releaseMedia(previousSession.sessionId);
      playbackTiming(r.sessionId, 'prepared', started);
      void save().catch(e => setError(pluginError(e)));
      return r.choices.resolverRoute;
    } catch (e) {
      if (n !== sequence.current) return; setEpisode(current.current?.ep || ep); setError(pluginError(e));
      const available = (e as { playbackChoices?: PreparedMedia["choices"] })?.playbackChoices;
      if (available) setChoices(available);
      if (available?.external) {
        if (session.current) releaseMedia(session.current.sessionId);
        session.current = null; setMedia(null); setEpisode(ep);
        current.current = { ep, playlistId: listId };
      }
    } finally { if (n === sequence.current) { transitioning.current = false; setBusy(false); } }
  };
  useEffect(() => {
    if (!start || !detail || !active || !canPlay || startedFromSource.current) return;
    startedFromSource.current = true;
    let live = true;
    void (async () => {
      let list = detail.playlists.find(p => p.id === start.playlistId);
      const cursors = new Set<string>();
      while (list && !list.episodes.some(e => e.id === start.episodeId) && list.next && !cursors.has(list.next) && plugin.methods.includes("media.listEpisodes")) {
        cursors.add(list.next);
        const page = await metadataRequest.read<Page<Episode>>(plugin, 'media.listEpisodes', { mediaId, playlistId: list.id, cursor: list.next, limit: 100 });
        if (!live) return;
        const ids = new Set(list.episodes.map(e => e.id));
        list = { ...list, next: page.next, episodes: [...list.episodes, ...page.items.filter(e => { if (ids.has(e.id)) return false; ids.add(e.id); return true; })] };
      }
      if (!live) return;
      const ep = list?.episodes.find(e => e.id === start.episodeId);
      if (!list || !ep || ep.locked) { setError("候选分集已变化或不可播放，请重新选择分集。"); return; }
      const loadedList = list;
      setDetail(old => old && ({ ...old, playlists: old.playlists.map(p => p.id === loadedList.id ? loadedList : p) }));
      setPlaylist(list.id); void play(ep, "play", list.id, selection.current, start.positionSec);
    })().catch(e => { if (live) setError(pluginError(e)); });
    return () => { live = false; };
  }, [start, !!detail, active, canPlay]);
  const autoStarted = useRef(false);
  useEffect(() => {
    if (!playbackRequested || (!detail && !entryEpisodeId) || start || !active || !canPlay || autoStarted.current) return;
    autoStarted.current = true;
    if (homePreview && entryEpisodeId) { void play({ id: entryEpisodeId, index: 1 }, "play", undefined); return; }
    if (!detail) return;
    let live = true;
    void (async () => {
      const resume = entry?.lastProgress;
      let list = detail.playlists.find(p => p.id === resume?.playlistId) || detail.playlists[0];
      if (!list) { setError("暂无可播放分集"); return; }
      const wanted = entryEpisodeId || resume?.episodeId;
      const cursors = new Set<string>();
      while ((!list.episodes.length || (wanted && !list.episodes.some(e => e.id === wanted))) && list.next && !cursors.has(list.next) && plugin.methods.includes("media.listEpisodes")) {
        cursors.add(list.next);
        const page = await metadataRequest.read<Page<Episode>>(plugin, 'media.listEpisodes', {mediaId,playlistId:list.id,cursor:list.next,limit:100});
        if (!live) return;
        const known = new Set(list.episodes.map(e=>e.id));
        list={...list,next:page.next,episodes:[...list.episodes,...page.items.filter(e=>!known.has(e.id))]};
      }
      if (!live) return;
      const loaded = list;
      setDetail(old=>old && ({...old,playlists:old.playlists.map(p=>p.id===loaded.id?loaded:p)}));
      const first=list.episodes.find(e=>e.id===wanted) || list.episodes[0];
      setPlaylist(list.id);
      if (!first || first.locked) { setError(first?.lockReason || "请选择可播放的分集"); return; }
      void play(first,"play",list.id);
    })().catch(e=>{if(live)setError(pluginError(e));});
    return()=>{live=false;};
  }, [!!detail, start, active, canPlay, playbackRequested]);
  const selectPlayback = (update: Partial<PlaybackSelection>) => {
    const c = current.current; if (c) void play(c.ep, "switch", c.playlistId, { ...selection.current, ...update });
  };
  const failed = () => {
    if (busy) return;
    if (!active) { failedWhileHidden.current = true; return; }
    const c = current.current; if (!c) return;
    if (!refreshed.current) { refreshed.current = true; void play(c.ep, "refresh", c.playlistId); }
    else {
      const key = (s: PlaybackSelection) => JSON.stringify([s.serverId, s.streamIndex, s.resolverId, s.resolverStreamIndex]);
      attempted.current.add(key(selection.current));
      const alternatives: PlaybackSelection[] = [];
      if (choices?.resolver) for (const stream of choices.resolver.streams) alternatives.push({ ...selection.current, resolverStreamIndex: stream.index });
      if (choices) for (const stream of choices.source.streams) {
        if (stream.index !== selection.current.streamIndex) alternatives.push({ ...selection.current, streamIndex: stream.index, resolverStreamIndex: 0 });
      }
      const next = host.prefs.playbackFallback && alternatives.find(s => !attempted.current.has(key(s)));
      if (next) { attempted.current.add(key(next)); void play(c.ep, "fallback", c.playlistId, next); }
      else setError("播放失败，已尝试可用播放流，可手动切换线路或重试。");
    }
  };
  const currentList = detail?.playlists.find(p => p.id === playlist);
  const more = async () => {
    if (!currentList?.next) return; setBusy(true);
    try {
      const result = await metadataRequest.read<Page<Episode>>(plugin, 'media.listEpisodes', { mediaId, playlistId: playlist, cursor: currentList.next, limit: 100 });
      setDetail(d => d ? { ...d, playlists: d.playlists.map(p => p.id === playlist ? { ...p, episodes: [...p.episodes, ...result.items.filter(e => !p.episodes.some(old => old.id === e.id))], next: result.next } : p) } : d);
    } catch (e) { setError(pluginError(e)); } finally { setBusy(false); }
  };
  const chooseList = (id: string) => {
    setEpisodePage(0); setPlaylist(id); const old = current.current?.ep, list = detail?.playlists.find(p => p.id === id);
    const aligned = old && list && alignedEpisode(old, list.episodes);
    if (aligned) { setEpisode(aligned); setError("已按季集编号定位，请选择分集播放。"); }
    else { setEpisode(null); if (old) setError("无法唯一对齐季集编号，请重新选集。"); }
  };
  const resume = entry?.lastProgress;
  const cover = detail?.cover || entryItem?.cover;
  const poster = useAssetUrls(plugin.meta.id, cover ? [cover] : [])[0];
  const previousEpisode = currentList?.episodes[(currentList?.episodes.findIndex(e=>e.id===episode?.id) ?? 0)-1];
  const nextEpisode = currentList?.episodes[(currentList?.episodes.findIndex(e=>e.id===episode?.id) ?? -1)+1];
  const prepareNext = () => {
    if (!active || document.hidden || !fullPlayback || detailOpen || busy || !playing.current || !currentList || !session.current || current.current?.playlistId !== currentList.id || current.current?.ep.id !== episode?.id || bufferAhead.current < Math.min(12, durationRef.current > 0 ? Math.max(2,durationRef.current-position.current-.2) : 12)) return;
    if (!nextEpisode && currentList.next && plugin.methods.includes('media.listEpisodes')) {
      const cursor = currentList.next, listId = currentList.id;
      if (pagePending.current === JSON.stringify([listId, cursor])) return;
      pagePending.current = JSON.stringify([listId, cursor]);
      void metadataRequest.read<Page<Episode>>(plugin, 'media.listEpisodes', { mediaId, playlistId: listId, cursor, limit: 100 }).then(page => {
        setDetail(d => d && ({ ...d, playlists: d.playlists.map(p => p.id !== listId || p.next !== cursor ? p : {
          ...p, next: page.next === cursor ? null : page.next,
          episodes: [...p.episodes, ...page.items.filter(e => !p.episodes.some(old => old.id === e.id))],
        }) }));
      }).catch(() => {});
      return;
    }
    if (!nextEpisode || nextEpisode.locked) return;
    const chosen = { ...selection.current, streamIndex: 0, resolverStreamIndex: 0 };
    const { serverId, ...options } = chosen;
    prefetch.offer(preparationKey(nextEpisode, currentList.id, chosen), { id: plugin.meta.id, ...options,
      args: { mediaId, episodeId: nextEpisode.id, playlistId: currentList.id, serverId, reason: 'preload' },
    }, durationRef.current > 0 && durationRef.current - position.current < 60);
  };
  const advancing = useRef(false);
  const advanceEpisode = async (direction: number) => {
    if (busy || advancing.current || !active || !episode || !currentList) return;
    advancing.current = true;
    try {
      let target = direction > 0 ? nextEpisode : previousEpisode;
      if (!target && direction > 0 && currentList.next && plugin.methods.includes('media.listEpisodes')) {
        setBusy(true);
        const result = await metadataRequest.read<Page<Episode>>(plugin, 'media.listEpisodes', { mediaId, playlistId: currentList.id, cursor: currentList.next, limit: 100 });
        const added = result.items.filter(e => !currentList.episodes.some(old => old.id === e.id));
        setDetail(d => d && ({ ...d, playlists: d.playlists.map(p => p.id === currentList.id ? { ...p, episodes: [...p.episodes, ...added], next: result.next === currentList.next ? null : result.next } : p) }));
        target = added[0];
      }
      if (!viewActive.current) return;
      if (target && !target.locked) await play(target);
      else if (target?.locked) setError(target.lockReason || '该分集暂不可播放');
    } catch (e) { if ((e as { code?: string }).code !== 'CANCELLED') setError(pluginError(e)); }
    finally { advancing.current = false; setBusy(false); }
  };
  const refresh = () => { const c=current.current; if(c){refreshed.current=false;attempted.current.clear();void play(c.ep,"refresh",c.playlistId);} };
  const beginEpisode = (ep: Episode) => {
    setPlayIntent(v=>v+1);
    setFullPlayback(true); setDetailOpen(false); autoStarted.current = true;
    if (current.current?.ep.id === ep.id && current.current.playlistId === (playlist || undefined) && media) return;
    void play(ep);
  };
  const beginPlayback = () => {
    setPlayIntent(v=>v+1);
    if (current.current && current.current.playlistId !== (playlist || undefined)) {
      const first = currentList?.episodes.find(ep=>!ep.locked);
      if (first) beginEpisode(first);
      else setError("请先加载并选择可播放分集。");
      return;
    }
    setFullPlayback(true); setDetailOpen(false);
    if (!media && !busy) {
      if (current.current) { autoStarted.current = true; void play(current.current.ep, "refresh", current.current.playlistId); }
      else { autoStarted.current = false; }
    }
  };
  const highlighted = episode?.id || resume?.episodeId;
  const pagedEpisodes = currentList?.episodes.slice(episodePage * 50, (episodePage + 1) * 50) || [];
  const episodesPanel = <div className="hg-episode-picker"><div className="hg-section-title"><h3>选集</h3><span>{detail?.episodeCount ? `共 ${detail.episodeCount} 集` : `${currentList?.episodes.length || 0} 集`}</span></div>
    {(detail?.playlists.length || 0) > 1 && <div className="hpp-playlists" aria-label="播放列表">{detail?.playlists.map(p=><button key={p.id} aria-pressed={p.id===playlist} className={p.id===playlist?"selected":""} disabled={busy} onClick={()=>chooseList(p.id)}>{p.label||p.id}</button>)}</div>}
    {(currentList?.episodes.length || 0) > 50 && <div className="hg-episode-ranges" aria-label="分集范围">{Array.from({length:Math.ceil((currentList?.episodes.length || 0)/50)},(_,i)=><button key={i} aria-pressed={episodePage===i} onClick={()=>setEpisodePage(i)}>{i*50+1}–{Math.min((i+1)*50,currentList!.episodes.length)}</button>)}</div>}
    {[...new Set(pagedEpisodes.map(e=>e.season))].map(season=><section key={season??"none"}>{season!==undefined&&<h4>{detail?.seasons?.find(s=>s.number===season)?.label||`第 ${season} 季`}</h4>}<div className="hpp-episodes">{pagedEpisodes.filter(e=>e.season===season).map(ep=><button key={ep.id} title={ep.lockReason||ep.title} className={highlighted===ep.id?"selected":""} aria-label={ep.label||`第 ${ep.index} 集`} aria-pressed={highlighted===ep.id} disabled={busy||ep.locked||!canPlay} onClick={()=>beginEpisode(ep)}><span>{ep.episodeNumber??ep.index}</span>{highlighted===ep.id&&<i aria-hidden="true">▮▮▮</i>}{ep.locked&&<small>不可用</small>}</button>)}</div></section>)}
    {!currentList?.episodes.length && <p className="hg-episode-empty">{detail ? "暂无分集，可尝试其他播放列表" : "正在读取分集…"}</p>}
    {currentList?.next && plugin.methods.includes("media.listEpisodes") && <button className="hg-load-episodes" disabled={busy} onClick={()=>void more()}>加载更多分集</button>}</div>;
  const playbackPanel=<><Notice error={error}/><div className="hpp-playback-options" aria-label="播放选择">{choices && <>
    <PlaybackOptions stage="源" choices={choices.source} disabled={busy} onServer={serverId=>selectPlayback({serverId:serverId||undefined,streamIndex:0,resolverStreamIndex:0})} onStream={streamIndex=>selectPlayback({streamIndex,resolverStreamIndex:0})}/>
    {choices.resolver && <PlaybackOptions stage="解析器" choices={choices.resolver} disabled={busy} onServer={serverId=>selectPlayback({resolverServers:{...selection.current.resolverServers,[choices.resolver!.pluginId]:serverId},resolverStreamIndex:0})} onStream={resolverStreamIndex=>selectPlayback({resolverStreamIndex})}/>}</>}
    <button disabled={busy||!active} onClick={refresh}>刷新播放</button><button disabled={busy||!active} onClick={()=>selectPlayback({serverId:undefined,resolverId:undefined,streamIndex:0,resolverStreamIndex:0,resolverServers:{}})}>使用默认播放选择</button>
    <button disabled={busy||!detail} onClick={()=>setSwitching(true)}>换源</button>
    {choices?.resolverRoute && <ResolverChooser key={choices.resolverRoute.host} route={choices.resolverRoute} busy={busy||!active} select={(resolverId,resolverHost)=>{const c=current.current;return c?play(c.ep,"switch",c.playlistId,{...selection.current,resolverId,resolverHost,resolverStreamIndex:0}):Promise.resolve(undefined);}}/>}</div></>;
  const detailPanel=detail?<><div className="legacy-detail-summary">{poster&&<img src={poster} alt=""/>}<div><h2>{detail.title}</h2><p>{plugin.meta.name}</p>{detail.score!==undefined&&<strong>{detail.score.toFixed(1)} 分</strong>}<p>{detail.episodeCount||currentList?.episodes.length||0} 集</p></div></div><h3>简介</h3><p className="hpp-description">{detail.description||"暂无简介"}</p>
    <button onClick={()=>void host.put(plugin.meta.id,detail,{tracking:!entry?.tracking}).catch(e=>setError(pluginError(e)))}>{entry?.tracking?"取消追更":"追更"}</button>
    {resume&&<button disabled={busy} onClick={()=>{const list=detail.playlists.find(p=>p.id===resume.playlistId),ep=list?.episodes.find(e=>e.id===resume.episodeId);if(list&&ep){setPlaylist(list.id);void play(ep,"play",list.id);}else setError("上次分集暂未加载，请加载对应播放列表后选集。");}}>继续观看 · {Math.floor(resume.positionSec)} 秒</button>}
    {active&&<MediaInteractions plugin={plugin} detail={detail}/>}<button onClick={manage}>管理此插件</button></>:<p>{error||"正在读取作品…"}</p>;
  const favoriteAction = () => { const item = detail || entryItem; if (item) void host.put(plugin.meta.id,item,{favorite:!entry?.favorite}).catch(e=>setError(pluginError(e))); };
  return <div className={`legacy-media-page ${fullPlayback ? "hg-watching" : homePreview ? "hg-discovering" : "hg-detail-entry"}`}>
    {active && !busy && choices?.external && <div className="absolute inset-x-4 top-4 z-50"><ExternalPlayback key={choices.external.url} target={choices.external}/></div>}
    {playbackRequested &&
    <PluginPlayer {...presentation} playIntent={playIntent} mode={fullPlayback ? "full" : "preview"} onOpenDetail={openDetail} media={canPlay?media:null} preparing={busy && (transitioning.current || !media)} active={active && !detailOpen && !transitioning.current} position={position.current} error={error} onRetry={refresh}
      previousPreview={fullPlayback ? previousEpisode ? {title:previousEpisode.label || '上一集',poster} : undefined : presentation.previousPreview}
      nextPreview={fullPlayback ? nextEpisode ? {title:nextEpisode.label || '下一集',poster} : undefined : presentation.nextPreview}
      onPanelChange={panel=>{if(panel==='detail'||panel==='episodes')setNeedsDetail(true);}}
      title={detail?.title||entryItem?.title||presentation.title} author={detail?.author?.name||plugin.meta.name} description={detail?.description||entryItem?.description} score={detail?.score??entryItem?.score} poster={poster}
      qualityLabel={(choices?.resolver || choices?.source)?.streams.find(s=>s.index===(choices?.resolver || choices?.source)?.streamIndex)?.label}
      episodeLabel={episode?.label||String(episode?.episodeNumber??episode?.index??1)} episodeCount={detail?.episodeCount||entryItem?.episodeCount||currentList?.episodes.length}
      social={detail&&active?<MediaInteractions compact plugin={plugin} detail={detail}/>:undefined}
      onBack={fullPlayback ? leavePlayback : onBack} favorite={!!entry?.favorite} onFavorite={detail||entryItem?()=>void host.put(plugin.meta.id,(detail||entryItem)!,{favorite:!entry?.favorite}).catch(e=>setError(pluginError(e))):undefined}
      onPreviousEpisode={!busy&&previousEpisode&&!previousEpisode.locked?()=>void advanceEpisode(-1):undefined}
      onNextEpisode={!busy&&((nextEpisode&&!nextEpisode.locked)||(!nextEpisode&&currentList?.next&&plugin.methods.includes('media.listEpisodes')))?()=>void advanceEpisode(1):undefined}
      panels={{detail:detailPanel,episodes:episodesPanel,playback:playbackPanel,comments:plugin.methods.includes("comments.list")?<CommentsPanel embedded plugin={plugin} mediaId={mediaId} episodeId={current.current?.ep.id}/>:undefined}}
      onPosition={(t,duration)=>{if(transitioning.current || (media?.sessionId!==session.current?.sessionId || media?.version!==session.current?.version))return;position.current=t;durationRef.current=duration||0;progress.update(t,duration);prepareNext();if(Date.now()-lastSave.current>5000){lastSave.current=Date.now();void save().catch(e=>setError(pluginError(e)));}}}
      onBuffer={seconds=>{bufferAhead.current=seconds;prepareNext();}}
      onWaiting={()=>{playing.current=false;prefetch.stopWarm();}}
      onPlaying={()=>{if(transitioning.current || (media?.sessionId!==session.current?.sessionId || media?.version!==session.current?.version))return;playing.current=true;progress.playing();prepareNext();}}
      onStopped={()=>{playing.current=false;prefetch.stopWarm();if(transitioning.current || (media?.sessionId!==session.current?.sessionId || media?.version!==session.current?.version))return;progress.pause();void save().catch(e=>setError(pluginError(e)));}}
      onEnded={()=>{if(transitioning.current || (media?.sessionId!==session.current?.sessionId || media?.version!==session.current?.version))return;progress.end();void save().catch(()=>{});if(active&&host.prefs.autoNext){if(fullPlayback)void advanceEpisode(1);else presentation.onNext?.();}}} onFailure={failed}/>}
    {detailOpen && active && <MediaDetailSheet item={detail || entryItem || null} poster={poster} source={plugin.meta.name}
      favorite={!!entry?.favorite} onFavorite={favoriteAction} onClose={()=>{setDetailOpen(false);if(!homePreview&&!fullPlayback)onBack?.();}}
      onPlay={beginPlayback} busy={busy || !canPlay || !detail || (!media && !currentList?.episodes.some(ep=>!ep.locked) && !currentList?.next)}
      playLabel={busy ? "正在准备…" : episode ? `继续${episode.label || `第 ${episode.index} 集`}` : resume ? "继续观看" : "立即播放"}
      error={error} onRetry={()=>{setError("");setDetailRound(v=>v+1);}}>
      {episodesPanel}
      {detail && <div className="hg-detail-secondary"><button aria-pressed={!!entry?.tracking} onClick={()=>void host.put(plugin.meta.id,detail,{tracking:!entry?.tracking}).catch(e=>setError(pluginError(e)))}>{entry?.tracking ? "已追更" : "追更这部作品"}</button><details><summary>更多作品信息</summary><MediaInteractions plugin={plugin} detail={detail}/><button onClick={manage}>管理此片源</button></details></div>}
    </MediaDetailSheet>}
    {switching&&detail&&active&&<SourceSwitcher plugin={plugin} detail={detail} previousEpisode={current.current?.ep} getPosition={()=>position.current} beforeSwitch={save} onConfirm={onSwitch} onClose={()=>setSwitching(false)}/>}
  </div>;
}

function PlaybackOptions({ stage, choices, disabled, onServer, onStream }: {
  stage: string; choices: PlaybackChoices; disabled: boolean; onServer: (id: string) => void; onStream: (index: number) => void;
}) {
  return <div>
    {choices.servers.length > 1 && <label>{stage}播放线路<select aria-label={`${stage}播放线路`} disabled={disabled} value={choices.serverId || ""} onChange={e => onServer(e.target.value)}>
      <option value="">插件默认</option>{choices.servers.map(server => <option key={server.id} value={server.id}>{server.label || server.id}</option>)}
    </select></label>}
    {choices.streams.length > 1 && <label>{stage}清晰度 / 播放流<select aria-label={`${stage}播放流`} disabled={disabled} value={choices.streamIndex} onChange={e => onStream(Number(e.target.value))}>
      {choices.streams.map(stream => <option key={stream.index} value={stream.index}>{stream.label || [stream.height && `${stream.height}p`, stream.codec, stream.format || stream.kind].filter(Boolean).join(" · ") || `播放流 ${stream.index + 1}`}</option>)}
    </select></label>}
  </div>;
}
