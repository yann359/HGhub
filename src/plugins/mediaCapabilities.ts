import Hls from "hls.js";
import { platform } from "../environment";
import type { Codec } from "./protocol";

export interface MediaCapabilities {
  codecs: Codec[];
  formats: ("hls" | "progressive")[];
}

// Use exactly the path VideoSurface selects, including Hls.js's preference for
// ManagedMediaSource. OS decoder lists do not describe WebView playback.
export function hlsPlaybackPath(video: HTMLVideoElement): "mse" | "native" | "unsupported" {
  const native = !!(video.canPlayType("application/vnd.apple.mpegurl") || video.canPlayType("application/x-mpegURL"));
  if (platform === "ios" && native) return "native";
  if (Hls.isSupported()) return "mse";
  return native ? "native" : "unsupported";
}

export function probeMediaCapabilities(): MediaCapabilities {
  const video = document.createElement("video");
  const mse = Hls.getMediaSource();
  const hlsPath = hlsPlaybackPath(video);
  const candidates: [Codec, string[]][] = [
    ["h264", ["avc1.42E01E"]],
    ["hevc", ["hvc1.1.6.L93.B0", "hev1.1.6.L93.B0"]],
    ["av1", ["av01.0.04M.08"]],
  ];
  const fileAudio = video.canPlayType('audio/mp4; codecs="mp4a.40.2"') === "probably";
  const mseAudio = hlsPath === "mse" && !!mse?.isTypeSupported('audio/mp4; codecs="mp4a.40.2"');
  const progressive = candidates.filter(([, profiles]) => fileAudio && profiles.some(
    profile => video.canPlayType(`video/mp4; codecs="${profile}"`) === "probably",
  )).map(([codec]) => codec);
  const hls = hlsPath === "mse" ? candidates.filter(([, profiles]) => mseAudio && profiles.some(
    profile => mse?.isTypeSupported(`video/mp4; codecs="${profile}"`),
  )).map(([codec]) => codec) : hlsPath === "native" ? progressive : [];
  // The protocol has no per-format codec matrix. Declare the conservative
  // intersection of enabled paths so a plugin can choose either advertised format.
  const paths = [progressive, hls].filter(codecs => codecs.length > 0);
  return {
    codecs: candidates.map(([codec]) => codec).filter(codec => paths.length > 0 && paths.every(path => path.includes(codec))),
    formats: [...(hls.length ? ["hls" as const] : []), ...(progressive.length ? ["progressive" as const] : [])],
  };
}
