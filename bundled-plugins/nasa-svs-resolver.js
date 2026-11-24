export const meta = {
  id: "dev.hghub.nasa-svs-resolver",
  name: "NASA SVS 播放解析器",
  description: "通过 NASA 官方 API 获取已核查素材组的 MP4 和清晰度。",
  author: "HGhub iOS 定制版",
  homepage: "https://svs.gsfc.nasa.gov/help/",
  matches: ["svs.gsfc.nasa.gov"],
  updateUrl: "https://raw.githubusercontent.com/yann359/HGhub/codex/ios-liquid-glass-plugin/bundled-plugins/nasa-svs-resolver.js",
  protocol: 1,
  version: "1.0.0",
  kind: "resolver",
  requires: [{ type: "api", name: "http.fetch" }],
};

const assets = {
  "5187": {
    "376413": ["phases_2024_fancy_360p30.mp4", "phases_2024_fancy_720p30.mp4", "phases_2024_fancy_1080p30.mp4"],
    "376417": ["orbit_2024_1080p30.mp4"],
    "376419": ["distance_2024_360p30.mp4", "distance_2024_720p30.mp4", "distance_2024_1080p30.mp4"],
  },
  "11719": {
    "339682": ["2006_full_960_MASTER.mp4", "2006_full-1920-MASTER.mp4"],
    "339683": ["N_America-1280-MASTER_ipod_sm.mp4"],
    "339684": ["Africa-1280-MASTER_ipod_sm.mp4"],
    "339685": ["Himalayas-1280-MASTER_ipod_sm.mp4"],
  },
};

export default hg => ({
  resolver: {
    async resolve({ url, reason }, ctx) {
      const match = /^https:\/\/svs\.gsfc\.nasa\.gov\/(\d+)\/?#media_group_(\d+)$/.exec(url);
      const allowed = match && assets[match[1]]?.[match[2]];
      if (!allowed) throw new hg.PluginError("NOT_FOUND", "这个 NASA 素材组不在已审核片库中");
      if (!hg.env.media.formats.includes("progressive")) throw new hg.PluginError("UNSUPPORTED", "当前环境不支持 MP4 播放");
      const response = await hg.http.fetch({
        url: "https://svs.gsfc.nasa.gov/api/" + match[1] + "/",
        headers: { Accept: "application/json" },
        credentials: "omit",
        responseType: "json",
        cache: reason === "refresh" ? undefined : { ttlSec: 3600, staleIfErrorSec: 86400 },
        signal: ctx.signal,
      });
      if (!response.ok) throw new hg.PluginError("UPSTREAM_ERROR", "NASA SVS 返回 HTTP " + response.status);
      const groups = response.data?.media_groups;
      const group = Array.isArray(groups) && groups.find(group => String(group.id) === match[2]);
      if (!group || !Array.isArray(group.items)) throw new hg.PluginError("UPSTREAM_CHANGED", "NASA 素材组已变更");
      const streams = group.items.map(item => item.instance).filter(item => {
        return item?.media_type === "Movie" && allowed.includes(item.filename) &&
          /^https?:\/\/svs\.gsfc\.nasa\.gov\/vis\/[^?#]+\.mp4$/.test(item.url);
      }).map(item => ({
        url: item.url.replace(/^http:/, "https:"),
        format: "progressive", codec: "h264",
        height: item.height, label: item.height + "p",
      })).sort((a, b) => a.height - b.height);
      if (!streams.length) throw new hg.PluginError("UPSTREAM_CHANGED", "NASA 暂无已审核的 MP4 地址");
      return { streams };
    },
  },
});
