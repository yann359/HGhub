# 预置片源与授权范围

HGhub iOS 1.1.0 的源代码继续按 [PolyForm Noncommercial 1.0.0](../LICENSE) 提供。预置的是插件、清单和经过核查的作品目录。视频、海报与字幕由列出的公开上游提供。安装不需要外部片源列表，联网条件影响播放。

| 插件 | 实际内容与播放 | 许可和出处 |
| --- | --- | --- |
| Blender 开放电影 | 《大雄兔》全片。Mux 自适应 HLS 与 Internet Archive MP4 两条线路。《辛特尔》全片。Internet Archive 两种 H.264 画质，Blender 官方英语 SRT。 | Blender Foundation，CC BY 3.0。保持原片片尾署名，作品详情保留作者、许可和出处。[大雄兔授权](https://peach.blender.org/about/)、[辛特尔下载与字幕](https://durian.blender.org/download/)、[辛特尔存档许可](https://archive.org/details/Sintel)、[Mux 对开放电影授权的说明](https://www.mux.com/blog/youtube-viewers-dont-care-about-video-quality) |
| NASA SVS 科普影像 | 《2024 年的月相与天平动》含 3 个原站素材组。《地球二氧化碳的一年》含 4 个原站素材组。 | NASA SVS / NASA Goddard Space Flight Center，按原站公开使用条款收录。只纳入核查过的官方素材，不使用标明含第三方受限音乐的组。[使用说明](https://svs.gsfc.nasa.gov/#faq)、[月球影片及署名](https://svs.gsfc.nasa.gov/5187/)、[CO₂ 影片及署名](https://svs.gsfc.nasa.gov/11719/)、[NASA 媒体使用指南](https://www.nasa.gov/nasa-brand-center/images-and-media/) |
| NASA SVS 播放解析器 | 通过官方 HTTPS JSON API 查询预先审核的页面和媒体组。返回实际提供的 MP4 清晰度。关闭此解析器会影响 NASA 片源，Blender 独立可用。 | [NASA SVS API 文档](https://svs.gsfc.nasa.gov/help/) |

## 覆盖

- 动画和欧美开放短片覆盖《大雄兔》《辛特尔》。
- 科普纪实影像覆盖 NASA 的月球和地球主题。分集对应上游原始素材组编号。
- 国剧、欧美商业电影和美剧、韩剧、日剧、日本商业动漫、综艺、商业短剧与直播仍缺少可以验证并预置的授权来源。应用保留对兼容 HPP 插件的导入能力。
- 以上默认源无需账号、密钥和登录 Cookie。域名、视频和文件路径都在独立插件中。上游停止供给时，播放器提供错误提示，其他插件可继续使用。

## 插件管理

首次启动自动安装启用两组片源与一个解析器。卸载后可以从本地目录批选清单和 JS 文件，也可以用 HTTPS 清单恢复。清单 URL 可以保存为订阅，插件中心可检查更新。自动安装由设置开关控制，播放中的更新按上游 HPP 规则延后。

[查看插件清单](https://raw.githubusercontent.com/yann359/HGhub/codex/ios-liquid-glass-plugin/bundled-plugins/manifest.json)

## 已排除的候选

- 《Sita Sings the Blues》的创作者采用 CC0，原版片中仍有另行授权的歌曲。片库未采用该片。[作品授权说明](https://www.sitasingstheblues.com/totalcompliance.html)
- NASA 月相页面另有配乐版本。片库仅挑选核查过的可视化组。
- 上游 HPP 的网页登录为可选能力。当前 iOS 适配器不声明 webview，显式要求它的插件会标为不兼容。当前片库无需这一能力。
