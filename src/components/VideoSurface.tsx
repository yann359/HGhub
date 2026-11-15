import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import Hls from "hls.js";
import { invoke } from "@tauri-apps/api/core";
import { platform } from "../environment";
import { inPictureInPicture } from "../mediaCapabilities";
import { pluginCommand, type PreparedMedia } from "../plugins/client";

export interface VideoHandle {
  el: () => HTMLVideoElement | null;
  play: () => void; pause: () => void; toggle: () => void;
  seek: (time: number, source?: string) => void;
  rate: (rate: number) => void; mute: (muted: boolean) => void; volume: (volume: number) => void;
}
interface Props {
  media: PreparedMedia; position: number; active: boolean; autoPlay?: boolean;
  poster?: string; muted: boolean; volume: number; rate: number; fit: "contain" | "cover";
  scale: number; panX: number; panY: number; isGesturing: boolean;
  onClick: () => void; onTime: (t: number) => void; onDuration: (d: number) => void;
  onSize: (w: number,h: number) => void; onBuffered: (fraction: number) => void;
  onPlaying: () => void; onPause: () => void; onWaiting: () => void;
  onEnded?: () => void; onError: () => void;
}

/** All requests keep the provider-owned HTTP transport, credentials and cancellation. */
export default forwardRef<VideoHandle, Props>(function VideoSurface(props, forwarded) {
  const ref = useRef<HTMLVideoElement>(null), latest = useRef(props), hls = useRef<Hls | null>(null);
  const wanted = useRef(props.autoPlay !== false), sourceEpoch = useRef(0), systemPause = useRef(false);
  latest.current = props;
  const available = () => latest.current.active && (!document.hidden || !!ref.current && inPictureInPicture(ref.current));
  const log = (event: string, extra = "") => {
    // Provider URLs, credentials and session capabilities must never enter logs.
    void invoke("debug_log", { msg: `[player] ${event} ${extra}` }).catch(() => {});
  };
  const play = () => {
    const video = ref.current;
    if (!video || !available()) return;
    wanted.current = true;
    const epoch = sourceEpoch.current;
    if (hls.current?.levels.length && !hls.current.loadingEnabled) hls.current.startLoad(video.currentTime);
    void video.play().then(() => {
      if (sourceEpoch.current !== epoch) return;
      if (!available()) video.pause();
    }).catch(error => {
      if (sourceEpoch.current !== epoch) return;
      log("play.rejected", error?.name || "unknown");
      if (error?.name === "NotAllowedError") wanted.current = false;
      latest.current.onPause();
    });
  };
  const pause = () => { wanted.current = false; ref.current?.pause(); };
  useImperativeHandle(forwarded, () => ({
    el: () => ref.current, play, pause,
    toggle: () => { if (ref.current?.paused) play(); else pause(); },
    seek: time => {
      const video = ref.current;
      if (!video || !available() || !Number.isFinite(time) || !Number.isFinite(video.duration) || video.duration <= 0) return;
      const target = Math.max(0, Math.min(video.duration - .15, time));
      try {
        if (hls.current?.levels.length) hls.current.startLoad(target);
        video.currentTime = target; latest.current.onTime(target);
      } catch { log("seek.rejected"); latest.current.onPause(); }
    },
    rate: rate => { if (ref.current) ref.current.playbackRate = rate; },
    mute: muted => { if (ref.current) ref.current.muted = muted; },
    volume: volume => { if (ref.current) ref.current.volume = volume; },
  }), []);
  useEffect(() => {
    const video = ref.current!, initial = latest.current;
    let disposed = false, failed = false, metadataApplied = false, recoveries = 0;
    let loadTimer: number | undefined, recoveryTimer: number | undefined;
    sourceEpoch.current++; wanted.current = initial.autoPlay !== false;
    const activity = (playing: boolean) => void pluginCommand("media.activity", { sessionId: initial.media.sessionId, playing })
      .then(() => { if (!playing) window.dispatchEvent(new Event("hpp.playback.idle")); }).catch(() => {});
    const fail = () => {
      if (disposed || failed) return;
      failed = true; clearTimeout(loadTimer); clearTimeout(recoveryTimer); hls.current?.stopLoad();
      log("source.failed", initial.media.format); activity(false); latest.current.onError();
    };
    const watchdog = () => {
      clearTimeout(loadTimer);
      if (available() && wanted.current) loadTimer = window.setTimeout(() => { if (available() && wanted.current) fail(); }, 30000);
    };
    const metadata = () => {
      if (!metadataApplied && initial.position > 0 && Number.isFinite(video.duration)) {
        metadataApplied = true;
        try { video.currentTime = Math.min(initial.position, Math.max(0, video.duration - .15)); }
        catch { log("resume.rejected"); }
      }
      if (wanted.current) play();
    };
    const playing = () => {
      if (disposed || !available()) { video.pause(); return; }
      systemPause.current = false;
      clearTimeout(loadTimer); recoveries = 0; activity(true); latest.current.onPlaying();
    };
    const paused = () => {
      if (disposed) return;
      clearTimeout(loadTimer); activity(false); latest.current.onTime(video.currentTime);
      if (available() && !systemPause.current && video.readyState >= 2 && !video.seeking) wanted.current = false;
      if (!video.ended) latest.current.onPause();
    };
    const visibility = () => {
      if (!available()) { systemPause.current = true; clearTimeout(loadTimer); video.pause(); hls.current?.stopLoad(); activity(false); }
      else if (wanted.current) { play(); watchdog(); }
    };
    const guard = () => { if (!available()) video.pause(); else wanted.current = true; };
    video.muted = initial.muted; video.volume = initial.volume; video.playbackRate = initial.rate;
    video.addEventListener("loadedmetadata", metadata); video.addEventListener("error", fail);
    video.addEventListener("play", guard); video.addEventListener("playing", playing); video.addEventListener("pause", paused);
    video.addEventListener("waiting", watchdog); video.addEventListener("seeking", watchdog);
    document.addEventListener("visibilitychange", visibility);
    const nativeHls = platform === "ios" && !!video.canPlayType("application/vnd.apple.mpegurl");
    if (initial.media.format === "hls" && !nativeHls && Hls.isSupported()) {
      const engine = new Hls({
        enableWorker: platform !== "ios", preferManagedMediaSource: false,
        autoStartLoad: initial.active, maxBufferLength: 30, maxMaxBufferLength: 60,
        backBufferLength: 20, maxBufferSize: 30 * 1024 * 1024,
        interstitialsController: undefined, contentSteeringController: undefined, enableInterstitialPlayback: false,
      });
      hls.current = engine; engine.loadSource(initial.media.url); engine.attachMedia(video);
      engine.on(Hls.Events.ERROR, (_, data) => {
        if (!data.fatal || disposed || !available()) return;
        if (recoveries++ >= 2) { fail(); return; }
        if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
          clearTimeout(recoveryTimer);
          recoveryTimer = window.setTimeout(() => { if (!disposed && available()) engine.startLoad(video.currentTime); }, 500 * recoveries);
        } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) engine.recoverMediaError();
        else fail();
      });
      log("source.load", "hls.js");
    } else { video.src = initial.media.url; log("source.load", nativeHls ? "native-hls" : "native-progressive"); }
    watchdog();
    const expiry = initial.media.playback.expiresAt;
    const expiryTimer = expiry ? window.setTimeout(fail, Math.max(1000, Math.min(2_147_483_647, expiry - Date.now() - 5000))) : undefined;
    return () => {
      disposed = true; sourceEpoch.current++;
      clearTimeout(expiryTimer); clearTimeout(loadTimer); clearTimeout(recoveryTimer);
      document.removeEventListener("visibilitychange", visibility);
      video.removeEventListener("loadedmetadata", metadata); video.removeEventListener("error", fail);
      video.removeEventListener("play", guard); video.removeEventListener("playing", playing); video.removeEventListener("pause", paused);
      video.removeEventListener("waiting", watchdog); video.removeEventListener("seeking", watchdog);
      hls.current?.destroy(); hls.current = null; video.pause(); video.removeAttribute("src"); video.load(); activity(false);
    };
  }, [props.media.sessionId]);
  useEffect(() => { const video = ref.current; if (video) { video.muted = props.muted; video.volume = props.volume; video.playbackRate = props.rate; } }, [props.muted, props.volume, props.rate]);
  useEffect(() => {
    if (!props.active) { systemPause.current = true; ref.current?.pause(); hls.current?.stopLoad(); }
    else if (wanted.current && ref.current?.readyState) play();
  }, [props.active]);
  const dimensions = () => {
    const video = ref.current!;
    if (Number.isFinite(video.duration)) latest.current.onDuration(video.duration);
    if (video.videoWidth) latest.current.onSize(video.videoWidth, video.videoHeight);
  };
  return <div className="absolute inset-0 overflow-hidden" onClick={props.onClick}>
    <video ref={ref} playsInline preload="metadata" crossOrigin="anonymous" poster={props.poster} className="w-full h-full"
      style={{ objectFit: props.fit, transform: `translate(${props.panX}px, ${props.panY}px) scale(${props.scale})`, transition: props.isGesturing ? "none" : "transform .18s ease" }}
      onLoadedMetadata={dimensions} onLoadedData={dimensions} onTimeUpdate={event => props.onTime(event.currentTarget.currentTime)}
      onProgress={event => { const video = event.currentTarget; props.onBuffered(video.duration && video.buffered.length ? video.buffered.end(video.buffered.length - 1) / video.duration : 0); }}
      onWaiting={props.onWaiting} onSeeking={props.onWaiting} onSeeked={() => { if (!ref.current?.paused) props.onPlaying(); else props.onPause(); }} onEnded={props.onEnded}>
      {props.media.playback.subtitles?.filter(t => t.format === "vtt").map((track, index) => <track key={track.url} kind="subtitles" src={track.url} label={track.label || track.language || `字幕 ${index + 1}`} srcLang={track.language} default={track.default}/>)}
    </video>
  </div>;
});
