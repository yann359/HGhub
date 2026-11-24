import { useEffect, useRef, useState } from "react";
import { pluginError, releaseMedia, type InstalledPlugin, type PlaybackChoices, type PreparedMedia } from "./client";
import { initialPlaybackSelection, rememberPlaybackSelection, type PlaybackSelection } from "./playbackSelection";
import { usePluginRequests } from "./usePluginRequests";
import PluginPlayer from "./PluginPlayer";
import { Notice } from "./BrowsePages";
import ResolverChooser from "./ResolverChooser";
import ExternalPlayback from './ExternalPlayback';

/** Live, article and feed playback share the provider-owned proxy and resolver chain. */
export default function ContentPlayer({ plugin, action, contentId, active = true, episodeId, playlistId, initialPosition = 0, onPosition, onPlaying, onStopped, onEnded }: {
  plugin: InstalledPlugin; action: "live.prepare" | "article.prepare" | "media.prepare"; contentId: string; active?: boolean;
  episodeId?: string; playlistId?: string; initialPosition?: number;
  onPosition?: (position: number, duration?: number) => void; onPlaying?: () => void; onStopped?: () => void; onEnded?: () => void;
}) {
  const request = usePluginRequests(), epoch = useRef(0), session = useRef<PreparedMedia | null>(null);
  const selection = useRef(initialPlaybackSelection(plugin.meta.id)), position = useRef(initialPosition), refreshed = useRef(false);
  const [media, setMedia] = useState<PreparedMedia | null>(null), [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [choices, setChoices] = useState<PreparedMedia["choices"] | null>(null);
  const prepare = async (reason: "play" | "refresh", chosen = selection.current) => {
    const n = ++epoch.current; request.cancelAll(); setBusy(true); setError(""); setChoices(null);
    const previous = session.current;
    try {
      const { serverId, ...options } = chosen;
      const result = reason === 'refresh' && previous ? await request<PreparedMedia>('media.refresh', { sessionId: previous.sessionId, version: previous.version }) : await request<PreparedMedia>(action, { id: plugin.meta.id, ...options, args: {
        ...(action === "live.prepare" ? { channelId: contentId } : action === "article.prepare" ? { articleId: contentId } : { mediaId: contentId, episodeId, playlistId }), serverId, reason,
      } });
      if (epoch.current !== n) { if (result.sessionId !== previous?.sessionId) releaseMedia(result.sessionId); return; }
      selection.current = rememberPlaybackSelection(result, chosen); session.current = result;
      setMedia(result); setChoices(result.choices);
      if (previous && previous.sessionId !== result.sessionId) releaseMedia(previous.sessionId);
      return result.choices.resolverRoute;
    } catch (e) {
      if (epoch.current !== n) return;
      setError(pluginError(e));
      const failure = e as { playbackChoices?: PreparedMedia["choices"] };
      if (failure.playbackChoices) setChoices(failure.playbackChoices);
      if (failure.playbackChoices?.external) {
        if (session.current) releaseMedia(session.current.sessionId);
        session.current = null; setMedia(null);
      }
    } finally { if (epoch.current === n) setBusy(false); }
  };
  useEffect(() => {
    if (active) { refreshed.current = false; void prepare("play"); }
    else setMedia(null);
    return () => { epoch.current++; request.cancelAll(); if (session.current) releaseMedia(session.current.sessionId); session.current = null; };
  }, [active, contentId, episodeId, playlistId, plugin.revision]);
  const change = (update: Partial<PlaybackSelection>) => { refreshed.current = false; void prepare("play", { ...selection.current, ...update }); };
  return <section aria-label={action === "live.prepare" ? "直播播放器" : action === "article.prepare" ? "图文视频播放器" : "信息流播放器"}>
    {active && !busy && choices?.external && <ExternalPlayback key={choices.external.url} target={choices.external}/>}
    <Notice error={error}/><div className="hpp-player-box legacy-inline-player">{media && active ? <PluginPlayer media={media} preparing={busy} active={!busy} position={action === "live.prepare" ? 0 : position.current} onPosition={(t, duration) => { if (media.sessionId !== session.current?.sessionId || media.version !== session.current?.version) return; position.current = t; onPosition?.(t, duration); }} onPlaying={onPlaying} onStopped={onStopped} onEnded={onEnded} onFailure={() => {
      if (busy) return;
      if (!refreshed.current) { refreshed.current = true; void prepare("refresh"); }
      else setError("播放失败，请切换播放流或手动重试。");
    }}/> : <div className="hpp-player-empty">{busy ? "正在解析播放地址…" : "等待播放"}</div>}</div>
    <details className="hpp-content-settings" open={error ? true : undefined}><summary>播放设置</summary><div className="hpp-playback-options">{choices && <><ContentPlaybackOptions label="源" choices={choices.source} disabled={busy} server={serverId => change({ serverId: serverId || undefined, streamIndex: 0, resolverStreamIndex: 0 })} stream={streamIndex => change({ streamIndex, resolverStreamIndex: 0 })}/>
      {choices.resolver && <ContentPlaybackOptions label="解析器" choices={choices.resolver} disabled={busy} server={serverId => change({ resolverServers: { ...selection.current.resolverServers, [choices.resolver!.pluginId]: serverId }, resolverStreamIndex: 0 })} stream={resolverStreamIndex => change({ resolverStreamIndex })}/>}</>}
      <button disabled={busy || !active} onClick={() => { refreshed.current = false; void prepare("refresh"); }}>重试播放</button>
    </div>
    {choices?.resolverRoute && <ResolverChooser key={choices.resolverRoute.host} route={choices.resolverRoute} busy={busy || !active} select={(resolverId, resolverHost) => {
      refreshed.current = false;
      return prepare("play", { ...selection.current, resolverId, resolverHost, resolverStreamIndex: 0 });
    }}/>}
    </details>
  </section>;
}
function ContentPlaybackOptions({ label, choices, disabled, server, stream }: {
  label: string; choices: PlaybackChoices; disabled: boolean; server: (id: string) => void; stream: (index: number) => void;
}) {
  return <div>{choices.servers.length > 1 && <label>{label}播放线路<select aria-label={`${label}播放线路`} disabled={disabled} value={choices.serverId || ""} onChange={e => server(e.target.value)}><option value="">插件默认</option>{choices.servers.map(s => <option key={s.id} value={s.id}>{s.label || s.id}</option>)}</select></label>}
    {choices.streams.length > 1 && <label>{label}播放流<select aria-label={`${label}播放流`} disabled={disabled} value={choices.streamIndex} onChange={e => stream(Number(e.target.value))}>{choices.streams.map(s => <option key={s.index} value={s.index}>{s.label || `${s.height ? `${s.height}p · ` : ""}${s.format || s.kind || "播放流"}`}</option>)}</select></label>}</div>;
}
