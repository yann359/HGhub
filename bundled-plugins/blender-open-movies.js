export const meta = {
  id: "dev.hghub.blender-open-movies",
  name: "Blender 开放电影",
  description: "Blender Foundation 开放短片，按 CC BY 3.0 提供署名与出处。",
  author: "HGhub iOS 定制版",
  homepage: "https://studio.blender.org/films/",
  updateUrl: "https://raw.githubusercontent.com/yann359/HGhub/codex/ios-liquid-glass-plugin/bundled-plugins/blender-open-movies.js",
  protocol: 1,
  version: "1.0.0",
  kind: "source",
};

const attribution = "© Blender Foundation · CC BY 3.0 · https://creativecommons.org/licenses/by/3.0/";
const titles = [
  {
    item: {
      id: "big-buck-bunny", title: "大雄兔（Big Buck Bunny）", type: "anime", year: 2008,
      status: "finished", episodeCount: 1, tags: ["动画", "喜剧", "短片", "CC BY 3.0"],
      cover: { url: "https://peach.blender.org/wp-content/uploads/bbb-splash.png" },
      description: "一只好脾气的大兔子遇上三只爱恶作剧的小动物。" + "\n\n" + attribution + "\nhttps://peach.blender.org/about/",
    },
    servers: [{ id: "mux", label: "Mux · 自适应 HLS" }, { id: "archive", label: "Internet Archive · MP4" }],
    streams: [
      { url: "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8", format: "hls", codec: "h264", serverId: "mux", label: "自适应" },
      { url: "https://archive.org/download/BigBuckBunny_328/BigBuckBunny_512kb.mp4", format: "progressive", codec: "h264", serverId: "archive", label: "节省流量" },
    ],
  },
  {
    item: {
      id: "sintel", title: "辛特尔（Sintel）", type: "anime", year: 2010,
      status: "finished", episodeCount: 1, tags: ["动画", "奇幻", "短片", "CC BY 3.0"],
      cover: { url: "https://archive.org/download/Sintel/Poster.jpg" },
      description: "年轻的辛特尔踏上旅途，寻找曾经照顾过的小龙。" + "\n\n" + attribution + "\nhttps://durian.blender.org/about/",
    },
    streams: [
      { url: "https://archive.org/download/Sintel/sintel-2048-stereo.mp4", format: "progressive", codec: "h264", height: 872, label: "原始立体声 · 872p" },
      { url: "https://archive.org/download/Sintel/sintel-2048-stereo_512kb.mp4", format: "progressive", codec: "h264", height: 240, label: "节省流量 · 240p" },
    ],
    subtitles: [{ url: "https://durian.blender.org/wp-content/content/subtitles/sintel_en.srt", format: "srt", language: "en", label: "English" }],
  },
];

const cards = titles.map(({ item }) => item);
const page = items => ({ items, next: null, total: items.length });

export default hg => {
  const find = id => {
    const title = titles.find(entry => entry.item.id === id);
    if (!title) throw new hg.PluginError("NOT_FOUND", "这部开放电影不在片库中");
    return title;
  };
  return {
    home: { getSections: () => page([{ id: "open-movies", title: "Blender 开放电影", layout: "row", items: cards }]) },
    catalog: {
      getCategories: () => [{ id: "open-movies", label: "动画短片" }],
      list: ({ categoryId }) => page(categoryId === "open-movies" ? cards : []),
    },
    search: {
      search: ({ query }) => page(cards.filter(item => [item.title, ...item.tags].some(text => text.toLowerCase().includes(query.trim().toLowerCase())))),
    },
    media: {
      getDetail({ id }) {
        const { item } = find(id);
        return { ...item, playlists: [{ id: "film", label: "正片", kind: "main", episodes: [{ id: item.id, index: 1, label: "正片" }] }] };
      },
      resolvePlayback({ mediaId, episodeId, serverId }) {
        const { streams, servers, subtitles } = find(mediaId);
        if (episodeId !== mediaId) throw new hg.PluginError("NOT_FOUND", "正片不存在");
        const formats = hg.env.media.formats;
        const available = streams.filter(stream => formats.includes(stream.format));
        const selected = serverId ? available.filter(stream => stream.serverId === serverId) : available;
        if (!selected.length) throw new hg.PluginError("UNSUPPORTED", "当前播放环境不支持这条线路");
        return { streams: selected, servers: servers?.filter(server => available.some(stream => stream.serverId === server.id)), subtitles };
      },
    },
  };
};
