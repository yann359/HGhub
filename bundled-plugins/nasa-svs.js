export const meta = {
  id: "dev.hghub.nasa-svs",
  name: "NASA SVS 科普影像",
  description: "NASA 科学可视化工作室的公开地球与月球影像，收录已审核的素材组。",
  author: "HGhub iOS 定制版",
  homepage: "https://svs.gsfc.nasa.gov/",
  linkDomains: ["svs.gsfc.nasa.gov"],
  updateUrl: "https://raw.githubusercontent.com/yann359/HGhub/codex/ios-liquid-glass-plugin/bundled-plugins/nasa-svs.js",
  protocol: 1,
  version: "1.0.0",
  kind: "source",
};

const base = "https://svs.gsfc.nasa.gov";
const titles = [
  {
    category: "moon", aliases: "Moon Phase and Libration 2024 月相 月球 地月距离",
    item: {
      id: "5187", title: "2024 年的月相与天平动", type: "documentary", year: 2023, status: "finished",
      cover: { url: base + "/vis/a000000/a005000/a005048/comp.0001_print.jpg" },
      description: "以科学可视化展示 2024 年逐小时的月相、视运动与地月距离。\n\n来源 NASA Scientific Visualization Studio / Ernie Wright。\nhttps://svs.gsfc.nasa.gov/5187/",
    },
    groups: [["376413", "月相与天平动"], ["376417", "月球轨道"], ["376419", "地月距离"]],
  },
  {
    category: "earth", aliases: "A Year In The Life Of Earth's CO2 二氧化碳 气候 地球",
    item: {
      id: "11719", title: "地球二氧化碳的一年", type: "documentary", year: 2014, status: "finished",
      cover: { url: base + "/vis/a010000/a011700/a011719/Still-Jan_1_2006-1920x959_print.jpg" },
      description: "利用 NASA 的 GEOS-5 模型，观察 2006 年大气中二氧化碳随季节和风场的变化。\n\n来源 NASA Goddard Space Flight Center / William Putman。\nhttps://svs.gsfc.nasa.gov/11719/",
    },
    groups: [["339682", "全球模拟"], ["339683", "北美"], ["339684", "非洲"], ["339685", "喜马拉雅"]],
  },
];
const cards = entries => entries.map(entry => ({ ...entry.item, episodeCount: entry.groups.length }));
const page = items => ({ items, next: null, total: items.length });

export default hg => {
  const find = id => {
    const entry = titles.find(title => title.item.id === id);
    if (!entry) throw new hg.PluginError("NOT_FOUND", "这部科普影像不在片库中");
    return entry;
  };
  return {
    home: { getSections: () => page([{ id: "science", title: "NASA 科普影像", layout: "row", imageAspect: "landscape", items: cards(titles) }]) },
    catalog: {
      getCategories: () => [{ id: "moon", label: "月球" }, { id: "earth", label: "地球与气候" }],
      list: ({ categoryId }) => page(cards(titles.filter(entry => entry.category === categoryId))),
    },
    search: {
      search: ({ query }) => page(cards(titles.filter(entry => (entry.item.title + " " + entry.aliases).toLowerCase().includes(query.trim().toLowerCase())))),
    },
    media: {
      getDetail({ id }) {
        const entry = find(id);
        const episodes = entry.groups.map(([episodeId, label], index) => ({ id: episodeId, index: index + 1, label }));
        return { ...cards([entry])[0], playlists: [{ id: "svs", label: "官方可视化", kind: "main", episodes }] };
      },
      matchUrl({ url }) {
        const match = /^https:\/\/svs\.gsfc\.nasa\.gov\/(\d+)\/?(?:#media_group_(\d+))?$/.exec(url);
        const entry = match && titles.find(title => title.item.id === match[1]);
        if (!entry || match[2] && !entry.groups.some(([id]) => id === match[2])) return null;
        return { mediaId: entry.item.id, episodeId: match[2] };
      },
      resolvePlayback({ mediaId, episodeId }) {
        if (!find(mediaId).groups.some(([id]) => id === episodeId)) throw new hg.PluginError("NOT_FOUND", "素材组不存在");
        return { streams: [{ url: base + "/" + mediaId + "/#media_group_" + episodeId, kind: "page", resolverId: "dev.hghub.nasa-svs-resolver" }] };
      },
    },
  };
};
