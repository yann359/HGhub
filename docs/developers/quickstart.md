# 制作第一个插件

交付一个 UTF-8 ES 模块，只导出 `meta` 和默认工厂。文件不超过 2 MiB，不使用外部 import。下载[完整虚构示例](../../examples/plugin-hello.js)，在插件设置填写自己控制的 HTTPS MP4 地址后验证播放。示例没有外部测试视频依赖。

```js
export const meta = { id: 'example.hello', name: '虚构示例', version: '1.0.0', protocol: 1 };
export default hg => ({
  media: {
    getDetail({ id }) {
      return { id, title: '示例短片', playlists: [{ id: 'main', episodes: [{ id: 'first', index: 1, label: '第一集' }] }] };
    },
    resolvePlayback() {
      throw new hg.PluginError('UNSUPPORTED', '请先配置自己的媒体地址');
    },
  },
  search: { search({ query }) { return { items: [{ id: 'demo', title: query || '示例短片' }], next: null }; } },
});
```

上面代码验证安装、搜索和详情。完整示例额外提供可配置的播放方法与首页。

## 接入站点

先观察正常访问站点时的请求与响应，记录列表、详情、分集和单次播放解析。不要猜接口或把会过期的播放地址当作品 ID。

```js
const response = await hg.http.fetch({
  url: 'https://api.example.com/search', query: { q: query },
  responseType: 'json', signal: ctx.signal,
});
if (!response.ok) throw new hg.PluginError('UPSTREAM_ERROR', '来源暂时不可用');
if (!Array.isArray(response.data.items)) throw new hg.PluginError('UPSTREAM_CHANGED', '来源结构已变化');
return { items: response.data.items.map(row => ({ id: String(row.id), title: row.title })), next: response.data.next || null };
```

占位域名必须替换为已验证的地址。保留真实作品、播放列表和分集 ID；index 仅表示顺序。分页游标应在插件重载后仍能使用。媒体及必要的请求头放入资源对象，由宿主处理。

## 播放地址刷新

上游地址会过期时，返回准确的毫秒 `expiresAt`。收到 `reason: "refresh"` 时重新取有效地址，不返回已知过期缓存。宿主会合并同一播放会话的并发刷新，从数据源开始重新执行当前解析链；成功后整体切换资源版本。正常播放、预加载和刷新均应透传 `ctx.signal`，取消后不要继续访问上游。字幕、请求头和有效期随新 `PlaybackInfo` 一起更新。

## 外部播放

无法在应用内播放时，返回 `{ kind: "external", url, external: { app: "播放器名称" } }`。允许 HTTP(S)、magnet、ed2k、rtsp、rtmp；`app` 和 `mime` 只是提示，不指定执行程序。宿主显示目标地址，用户点“确认打开”后交给系统默认处理程序，缺少程序时显示失败。外部打开不附加资源请求头、Cookie 或变换；不可依赖这些条件才能访问的链接不适合此路径。预加载不会打开外部应用。

## 字幕

在 `PlaybackInfo.subtitles` 中返回字幕地址和实际 `format`（`srt` 或 `vtt`），可带 `language`、`label`、`default` 与 `headers`。SRT 即使以 `application/octet-stream` 返回，宿主也按声明转换成 UTF-8 VTT；保留轨道信息供播放器选择。编码优先读取 BOM，其次响应的 `charset`，都没有时按 UTF-8。旧编码字幕需提供正确的 charset；不猜测无标记编码。字幕读取或转换失败只影响该轨，ASS 当前不渲染。

## 导入

打开“插件中心 → 导入与订阅”，选择文件或粘贴源码。确认状态“已就绪”，从搜索进入详情和选集。修改代码可同 ID、同版本重新导入；新修订会停止旧媒体会话。按[验证步骤](testing.md)检查正常路径、错误、取消、第二页与重载。
