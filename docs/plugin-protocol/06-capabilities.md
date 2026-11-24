# 06 · 能力模块

[← 总览](README.md)

插件对象由若干**能力命名空间**组成。某个命名空间存在，就表示插件实现了该能力；
宿主据此决定界面上展示哪些入口。

```js
export default function (hg) {
  return {
    media:  { getDetail, resolvePlayback },   // 点播源必需；纯直播源可只实现 live
    search: { search },                        // 可选：有它才有搜索
    home:   { getSections },                   // 可选：有它才有首页内容
  };
}
```

所有能力方法与 transformResource 都是 `(params, ctx)` 形状（见 [02 §5](02-runtime.md)）；
卸载钩子 dispose 为例外。数据类型见 [05](05-content-model.md)。
带 `?` 的方法在该能力内可选。

## 1. 能力一览

| 能力 | 方法 | 对应的界面 |
| --- | --- | --- |
| **`media`**（点播源必需） | `getDetail` `resolvePlayback` `listEpisodes?` `matchUrl?` | 详情页、播放器 |
| **`resolver`**（解析器必需） | `resolve` | 无独立界面；为数据源提供播放地址 |
| `home` | `getSections` `listSection?` | 首页（分区） |
| `feed` | `list` | 沉浸式信息流（竖屏连续播放） |
| `catalog` | `getCategories` `list` `getFilters?` | 分类页与筛选 |
| `search` | `search` `getFilters?` `suggest?` `trending?` | 搜索页、聚合搜索、换源 |
| `live` | `getGroups` `listChannels` `resolve` `getProgrammes?` | 直播频道 |
| `articles` | `list` `getDetail` | 图文 |
| `comments` | `list` `send?` `like?` | 评论面板 |
| `interact` | `like?` `favorite?` `follow?` | 点赞、收藏、关注按钮 |
| `auth` | `getMethods` `submit` `getProfile` `logout` | 插件页的登录表单 |
| `library` | `listMedia` `listFolders?` `listFollowing?` | 云端收藏与历史 |
| `progress` | `report` | 无界面（播放进度回报） |
| `updates` | `check` | 片库的"有更新"角标 |
| `endpoints` | `list` | 插件页的站点线路选择 |
| `ui` | `getActions` `submit` | 插件页的操作表单 |
| — | `transformResource?` `dispose?` | 顶层钩子（§16） |

- **数据源**（`meta.kind` 为 `source`，默认）至少实现 `media` 或 `live` 之一，可同时实现。
  实现 `media` 时**应当**至少有 `home`、`feed`、`catalog`、`search` 或 `media.matchUrl` 作为内容入口。
  纯直播源的入口为频道列表，不必伪造点播详情。返回点播 `MediaItem` 的能力要求同时实现 `media`。
- **解析器**（`meta.kind` 为 `resolver`）只需要 `resolver` 和 `meta.matches`。
- 宿主在初始化时校验每个命名空间的必需方法，并记录可选方法是否存在，界面按具体方法显示按钮；
  不能仅因存在空 `interact` 对象就显示全部交互。工厂应保持能力集合稳定，不随登录态改变；登录限制用错误码表达。

界面部分见 [08 宿主界面](08-frontend.md)。榜单没有独立页面：上游的排行榜用 `catalog` 的排序项
或 `home` 里的一个分区表达。

## 2. `media`（点播源必需）

```ts
media: {
  getDetail(params: { id: string }, ctx): MediaDetail

  resolvePlayback(params: {
    mediaId: string;
    episodeId: string;
    playlistId?: string;
    serverId?: string;                         // 用户选定的播放线路；省略由插件选默认
    reason: "play" | "preload" | "refresh";
    codecs: ("h264" | "hevc" | "av1")[];       // 当前播放路径实际可用，按偏好排序
    maxHeight?: number;
  }, ctx): PlaybackInfo

  listEpisodes?(params: PageParams & { mediaId: string; playlistId: string }, ctx): Page<Episode>

  matchUrl?(params: { url: string }, ctx): { mediaId: string; episodeId?: string } | null
}
```

- **`getDetail`**：返回 `MediaDetail`，其中 `playlists` 至少一项。
- **`resolvePlayback`**：返回至少一条 `streams`。
  - `reason: "play"` 用户即将观看；`"preload"` 宿主为下一条做的预取，插件可以同等对待；
    `"refresh"` 播放中途地址过期或取流失败后的重新解析，插件**不应**返回缓存的旧地址。
  - 当前宿主已接入下一集 `preload`：结果可能被取消，也可能直接用于播放，命中有效预备会话时不会再重复调用一次 `play`。因此预取应返回完整可用的 `PlaybackInfo`，准确声明 `expiresAt`，网络操作沿用 `ctx.signal`。
    预取不代表用户已经观看；支持进度回报的插件应以 `progress.report` 的实际播放事件记录观看。需要登录或选择时正常返回协议错误，预取不会弹出交互界面，用户选择该集后再走正常处理。有限字节预取的范围及未支持情况见 [实现说明](IMPLEMENTATION.md#连播与首播优化2026-11-16)。
  - 有多条播放线路时在 `servers` 中列出；`serverId` 给定时返回该线路的流。
  - 拿不到可直接播放的地址、只有播放页时，返回 `kind: "page"` 的流交给解析器（§3）。
  - 取不到任何地址时抛 `CONTENT_LOCKED`（上游限制）或 `UPSTREAM_CHANGED`（结构变了）。
- **`listEpisodes`**：仅当 `playlists[].next` 非空时由宿主调用，用于分集很多的内容。
- **`matchUrl`**：用户粘贴一个上游站点的链接时，把它映射成条目。
  插件在 `meta.linkDomains`（主机名数组）里声明自己认识哪些域名，宿主据此选插件，无需加载全部插件。
  不认识该地址时返回 `null`。

## 3. `resolver`（解析器）

大量影视源使用同一批视频托管商。把"某个托管商的播放页 → 可播放地址"做成独立插件，
多个数据源就能共用一个解析器。

```ts
meta: {
  kind: "resolver",
  matches: ["*.host-a.example", "player.host-b.example"],
  name: "某某解析器",
}

resolver: {
  resolve(params: {
    url: string;                               // 待解析的播放页地址
    headers?: Record<string, string>;          // 数据源给出的非凭据请求头
    serverId?: string;                         // 仅属于当前解析器的播放线路
    reason: "play" | "preload" | "refresh";
    codecs: ("h264" | "hevc" | "av1")[];
    maxHeight?: number;
  }, ctx): PlaybackInfo
}
```

`meta.matches` 是解析器**能处理**的主机模式（写法见下）。宿主的调用时机：

1. 数据源返回 `kind: "page"` 的流。
2. 宿主按主机选择解析器：流的 `resolverId` 指明了插件时用它；否则匹配 `matches`；
   命中多个时使用用户为该主机记住的选择，没有记忆则让用户选（可勾选"记住"）。
3. 宿主用 `url` 与数据源给出的非凭据请求头调用 `resolver.resolve`，把返回的流当作该集的流交给播放器。
4. 没有可用的解析器时，提示用户"该来源需要安装解析器"，并列出当前已安装的解析器供选择。

边界：

- 解析器与数据源互不可见：它拿不到数据源的存储、Cookie 与凭据，只拿到页面地址和请求头。
  `Authorization` 与 `Cookie` 请求头**不得**转交给解析器。
- 解析器自己的网络请求由它自己负责，与数据源的请求相互独立。
- 1.0 解析器只能返回 `media` 或 `external` 流，不得再返回 `page`；不做递归解析。
- 解析结果的资源归属、字幕与有效期合并、刷新顺序见 [09 §2](09-media-transport.md)；
  `resolverId` 不得指向停用、不兼容或初始化失败的插件，指定不可用时明确提示，不静默改用其他解析器。
- 页面地址可能含会话相关的查询参数（数据源自己生成的签名地址很常见）。
  数据源**应当**避免把长期有效的令牌放进交给解析器的地址里。

主机模式写法：

| 写法 | 匹配 |
| --- | --- |
| `api.example.com` | 仅该主机 |
| `*.example.com` | `example.com` 的任意层级子域，不含 `example.com` 本身 |
| `*` | 任意主机。只对解析器有意义，会让它在所有 `page` 流上成为候选 |

## 4. `home`

```ts
home: {
  getSections(params: PageParams, ctx): Page<Section>
  listSection?(params: PageParams & { sectionId: string }, ctx): Page<MediaItem>
}

interface Section {
  id: string;
  title?: string;
  layout: "banner" | "row" | "grid" | "list";
  imageAspect?: "portrait" | "landscape";   // 卡片用竖版封面还是横版图，默认 portrait
  items: MediaItem[];
  more?: boolean;        // 为 true 时显示"查看更多"，宿主调用 listSection
}
```

| `layout` | 呈现 |
| --- | --- |
| `banner` | 顶部轮播，使用 `backdrop`（缺失时回退 `cover`） |
| `row` | 横向滑动的一行卡片 |
| `grid` | 多列网格 |
| `list` | 带简介的纵向列表 |

- 分区本身可以分页（`getSections` 返回 `next`），适合"无限下拉的推荐流"。
- 布局是**提示**：宿主可以根据屏幕尺寸调整，但**应当**保持分区顺序。
- 宿主在首页综合模式下会把多个插件的分区拼接展示，并标注来源（见 [08 §3](08-frontend.md)）。

## 5. `feed`

```ts
feed: {
  list(params: PageParams, ctx): Page<MediaItem>
}
```

沉浸式信息流：每条内容全屏展示并自动播放。条目**应当**带 `entryEpisodeId`，
否则宿主需要先调用 `getDetail` 才能播放。适合短剧、短视频类上游。

## 6. `catalog`

```ts
catalog: {
  getCategories(params: {}, ctx): Category[]
  list(params: PageParams & {
    categoryId: string;
    filters?: Record<string, string[]>;   // 过滤器 id → 已选项 id
    sort?: string;
  }, ctx): Page<MediaItem>
  getFilters?(params: { categoryId: string }, ctx): { filters?: Filter[]; sorts?: FilterOption[] }
}

interface Category {
  id: string;
  label: string;
  children?: Category[];        // 至多两层
  filters?: Filter[];
  sorts?: FilterOption[];
}

interface Filter {
  id: string;
  label: string;
  options: FilterOption[];
  multiple?: boolean;           // 默认单选
}

interface FilterOption { id: string; label: string }
```

- 过滤器可以随分类静态给出（`Category.filters`），也可以按需取（`getFilters`，适合过滤项随分类变化的上游）。
  两者都有时以 `getFilters` 的结果为准。
- `sorts` 用于表达上游的排序方式（最新、最热、评分等）。上游的榜单可以做成一个固定的分类加一组排序项。
- 宿主**应当**缓存分类树，至少在一次会话内不重复请求。
- 分类页按插件分栏展示（见 [08 §4](08-frontend.md)）。

## 7. `search`

```ts
search: {
  search(params: PageParams & {
    query: string;
    filters?: Record<string, string[]>;
  }, ctx): Page<MediaItem>
  getFilters?(params: {}, ctx): Filter[]
  suggest?(params: { query: string }, ctx): string[]     // 输入联想
  trending?(params: {}, ctx): string[]                    // 热搜词
}
```

- `search` 是聚合搜索与换源的基础（§17）。希望参与聚合的插件**应当**保证它在数秒内返回。
- `suggest` 由宿主防抖后调用，且只针对当前插件，不跨插件聚合。
- 插件**不应**把搜索词写入日志。

## 8. `live`

```ts
live: {
  getGroups(params: {}, ctx): LiveGroup[]
  listChannels(params: PageParams & { groupId?: string }, ctx): Page<Channel>
  resolve(params: {
    channelId: string; serverId?: string; codecs: ("h264" | "hevc" | "av1")[];
    maxHeight?: number; reason: "play" | "preload" | "refresh";
  }, ctx): PlaybackInfo
  getProgrammes?(params: { channelId: string; from: number; to: number }, ctx): Programme[]
}

interface LiveGroup { id: string; label: string }

interface Channel {
  id: string;
  name: string;
  logo?: ImageRef;
  number?: number;           // 频道号
  groupId?: string;
  now?: string;              // 正在播出的节目名
}

interface Programme {
  title: string;
  startAt: number;
  endAt: number;
  description?: string;
}
```

- `resolve` 返回的 `streams` **应当**带 `live: true`。多个信号源用 `servers` 表达。
- `reason: "refresh"` 与点播相同，必须重新取得有效地址；直播频道可建立与点播相同的媒体会话和解析链。
- 节目单（`getProgrammes`）可选；宿主在用户打开频道详情时才请求。

## 9. `articles`

```ts
articles: {
  list(params: PageParams, ctx): Page<ArticleItem>
  getDetail(params: { id: string }, ctx): ArticleDetail
}

interface ArticleItem {
  id: string;
  title: string;
  summary?: string;
  cover?: ImageRef;
  tags?: string[];
  author?: Author;
  stats?: { likes?: number; comments?: number; favorites?: number; hot?: number };
  publishedAt?: number;
  mediaId?: string;          // 关联的作品，宿主提供跳转
}

interface ArticleDetail extends ArticleItem {
  contentHtml?: string;      // 富文本正文，宿主清洗后渲染（05 §9）
  contentText?: string;      // 纯文本正文
  streams?: Stream[];        // 文内附带的视频
  interaction?: { liked?: boolean; favorited?: boolean };
}
```

## 10. `comments`

```ts
comments: {
  list(params: PageParams & { target: CommentTarget }, ctx): Page<Comment>
  send?(params: { target: CommentTarget; text: string }, ctx): Comment | void
  like?(params: { commentId: string; on: boolean }, ctx): void
}

type CommentTarget =
  | { kind: "media"; mediaId: string; episodeId?: string }
  | { kind: "article"; articleId: string }
  | { kind: "replies"; commentId: string };      // 某条评论的回复

interface Comment {
  id: string;
  author: { id?: string; name: string; avatar?: ImageRef };
  text: string;
  createdAt?: number;
  likes?: number;
  liked?: boolean;
  replyCount?: number;
  replies?: Comment[];       // 内联的前几条回复
  pinned?: boolean;
  region?: string;           // 上游提供的属地文本
}
```

- 回复一条评论：`send` 的 `target` 用 `{ kind: "replies", commentId }`。
- `send` 返回新评论时宿主直接插入列表；不返回时宿主刷新列表。
- 插件不支持某种 `target` 时抛 `UNSUPPORTED`，宿主隐藏对应入口。

## 11. `interact`

```ts
interact: {
  like?(params: { target: InteractTarget; on: boolean }, ctx): void
  favorite?(params: { target: InteractTarget; on: boolean; folderId?: string }, ctx): void
  follow?(params: { authorId: string; on: boolean }, ctx): void
}

type InteractTarget =
  | { kind: "media"; mediaId: string; episodeId?: string }
  | { kind: "article"; articleId: string };
```

这些是**账号侧**的操作，会改动上游的数据，需要登录；未登录时插件抛 `AUTH_REQUIRED`。
宿主的本地收藏与历史不依赖它们（设计原则"本地优先"）。

## 12. `auth`

```ts
auth: {
  getMethods(params: {}, ctx): { methods: AuthMethod[]; registerUrl?: string }
  submit(params: { methodId: string; values: Record<string, string> }, ctx): AuthResult
  getProfile(params: {}, ctx): UserProfile | null
  logout(params: {}, ctx): void
}

interface AuthMethod {
  id: string;
  label: string;             // 如"账号密码登录""网页登录"
  form?: Form;               // 省略表示无需填表（如网页登录），宿主直接调用 submit
}

interface Form {
  title?: string;
  description?: string;
  fields: FormField[];
  submitLabel?: string;
}

interface FormField {
  key: string;
  type: "text" | "password" | "email" | "number" | "otp" | "select" | "hidden" | "captcha";
  label?: string;
  placeholder?: string;
  required?: boolean;
  value?: string;                                // 预填值
  options?: { value: string; label: string }[];  // select
  image?: ImageRef;                              // captcha 的题面图片
}

type AuthResult =
  | { status: "ok"; user: UserProfile }
  | { status: "next"; form: Form };              // 还需要一步（验证码、短信码等）

interface UserProfile {
  id: string;
  name: string;
  avatar?: ImageRef;
  description?: string;
  stats?: Record<string, number>;   // 展示用计数，键为展示标签，如 { "关注": 3 }
}
```

流程：

```text
宿主调用 getMethods → 用户在插件页的登录表单里选一种方式并填写 → submit
   ├─ { status: "ok" }   → 登录完成，宿主记录登录态并展示 user
   ├─ { status: "next" } → 宿主渲染新表单，再次 submit（此前各步的值与本步合并后一起提交）
   └─ 抛 AUTH_INVALID    → 宿主在表单内展示错误消息
```

- 插件自行取得的令牌可保存到 `hg.storage`，也可依赖普通 Cookie 罐。宿主不保存用户输入的密码。
- 若支持 `webview` 特性，可提供无 `form` 的登录方式。用户点击后，插件在 `auth.submit` 里调用 `hg.webview.open`。宿主管理该插件的同源登录会话，插件向账号接口读取 `UserProfile`。[网页登录流程](04-host-api.md#12-hgwebview可选)
- `getProfile`：用已保存的会话取回资料；未登录返回 `null`；会话失效抛 `AUTH_EXPIRED`。
  宿主在插件页与"我的"里调用它来展示登录态。
- `logout`：通知上游并清理本地会话；即使上游请求失败也**必须**完成本地清理。
- `registerUrl`：宿主在登录表单旁提供"注册"入口，用外部浏览器打开。
- 任何调用抛 `AUTH_EXPIRED` 时，宿主清除界面的登录状态缓存并引导重新登录；
  失效令牌由插件清理，不允许宿主猜测存储键或直接清空该插件全部数据。

## 13. 账号侧数据

### 13.1 `library`

```ts
library: {
  listMedia(params: PageParams & {
    kind: "favorites" | "likes" | "history";
    folderId?: string;
  }, ctx): Page<MediaItem>
  listFolders?(params: {}, ctx): { id: string; label: string; count?: number }[]
  listFollowing?(params: PageParams, ctx): Page<Author>
}
```

上游账号里的收藏、点赞、历史。宿主在"我的"里把它与本地片库分开展示，并提供"导入到本地"。

### 13.2 `progress`

```ts
progress: {
  report(params: {
    mediaId: string;
    episodeId: string;
    positionSec: number;
    durationSec?: number;
    event: "start" | "heartbeat" | "pause" | "end";
  }, ctx): void
}
```

向上游回报播放进度。宿主限频调用（见 [03 §3](03-host-rules.md)）；失败静默，不影响播放。

### 13.3 `updates`

```ts
updates: {
  check(params: {
    items: { mediaId: string; episodeCount?: number; updatedAt?: number }[];   // ≤ 50
  }, ctx): { mediaId: string; episodeCount?: number; updatedAt?: number; latestLabel?: string }[]
}
```

宿主把片库里"追更中"的条目连同已知状态批量交给插件，插件返回有变化的条目。
只返回有更新的即可；宿主据此显示角标。未实现该能力时，宿主在用户打开详情页时顺带比对。

## 14. `endpoints`

```ts
endpoints: {
  list(params: { refresh?: boolean }, ctx): Endpoint[]
}

interface Endpoint {
  id: string;
  label?: string;
  url: string;          // 纯 origin，如 "https://www.example.com"
}
```

用于上游有多个入口域名（主站、备用站、镜像）的情况。

- 列表的**第一项**是默认线路，id 与 origin 在列表内分别唯一。固定线路直接返回；动态列表可读持久缓存，
  无缓存的首次初始化允许用插件内的引导地址联网。`refresh: true` 时绕过旧列表缓存更新。
- 宿主在首次启用、设置变化后的重新初始化、用户打开线路面板时调用；`refresh: true` 仅在用户点击刷新时传入。
  `hg.endpoint.current` 可能为 `null`，本方法必须能在没有当前线路时返回非空列表，不依赖工厂联网。
- 无旧选择或旧选择已被移除时，初始化选择第一项。用户主动刷新移除了当前线路时，先停止相关会话、
  选择新默认线路并重建；普通打开面板可使用已缓存的列表。
- 宿主负责：展示列表、探测各线路的可达性（对线路根路径发起一次请求，只报结果，不自动切换）、
  保存用户的选择、切换后重启插件。
- 插件通过 `hg.endpoint.current` 读取当前线路（见 [04 §4](04-host-api.md)）。

## 15. `ui`

插件不能绘制界面。需要用户输入或操作时（切换账号、填写自定义参数、选择偏好），
插件声明**表单**，由宿主渲染在插件页里（见 [08 §2.4](08-frontend.md)）。

```ts
ui: {
  getActions(params: {}, ctx): ActionDescriptor[]
  submit(params: { actionId: string; values: Record<string, string> }, ctx): ActionResult
}

interface ActionDescriptor {
  id: string;
  label: string;             // 按钮文字，如"切换账号""自定义线路"
  description?: string;      // 按钮下方的说明
  form: Form;                // 与 auth 的 Form 同构（§12）
}

type ActionResult =
  | { status: "ok"; message?: string }     // 完成后可在界面上提示一句话
  | { status: "next"; form: Form };        // 还需要一步，宿主渲染新表单后再次 submit
```

- 宿主在插件页把每个动作渲染为一个按钮；点击后展开表单，提交后调用 `submit`。
- 动作是**用户触发**的：宿主只在这些时机调用 `getActions` 与 `submit`。
- 需要登录才能执行的动作，插件抛 `AUTH_REQUIRED`；宿主提示先登录。
- **不提供**调用中途弹窗提问的能力：插件在方法执行期间不能要求用户输入。
  网页登录的用户入口和完成条件见 [04 §12](04-host-api.md)。站点人机验证的独立声明与手动完成入口尚待定义。

## 16. 顶层钩子

```ts
transformResource?(params: {
  kind: "image" | "playlist" | "segment" | "key";
  name: string;              // 来自 ResourceTransform.name
  state?: Json;              // 来自 ResourceTransform.state
  url: string;               // 资源的最终地址
  contentType?: string;
  data: Uint8Array;
}, ctx): { data: Uint8Array; contentType?: string }

dispose?(): void
```

- **`transformResource`**：处理 `{ type: "plugin" }` 的字节变换（见 [05 §7.2](05-content-model.md)）。
  它在加载图片与播放时被高频调用，**应当**保持纯计算、不联网。
- 设置修改统一由宿主取消在途调用并重新初始化，不提供设置变更回调。
- **`dispose`**：卸载前的尽力同步通知（预算 1 秒），不保证调用；不得依赖它来保存状态。已被中断或无法继续执行的运行时跳过通知。通知时已没有业务调用作用域，不可发起网络、存储、Cookie、HTML 或定时器操作；可释放插件内存引用和写日志。宿主不等待其返回的 Promise，抛错或超时不阻止卸载。

## 17. 聚合搜索与换源

这是宿主功能，建立在各插件的 `search` 与 `media` 能力之上，不需要插件额外实现任何东西。
界面细节见 [08](08-frontend.md)。

### 17.1 聚合搜索

用户提交搜索后，宿主并行调用用户选中的插件的 `search`（单选、多选或全部）。
触发时机与并发上限见 [03 §3](03-host-rules.md)。

综合模式下，结果先按来源分栏，另有一个"全部"栏把同一部作品合并成一张卡片：

1. 归一化标题：去掉空格与标点、统一全角半角、忽略大小写。
2. 归一化后标题相同、且 `year` 与 `type` 不冲突的条目合并。
3. 合并后的卡片标注来源数量，点进去可以选择从哪个来源播放。

### 17.2 换源

当前来源无法播放（`CONTENT_LOCKED`、`NOT_FOUND`、`UPSTREAM_CHANGED`、取流失败）或用户主动选择时：

1. 宿主用当前条目的 `title`（及 `year`、`type`）在其他插件中搜索。
2. 按标题相似度排序，标题完全一致且年份、类型不冲突的排在前面。
3. **由用户确认**候选条目；宿主不自动替换。
4. 确认后，宿主建立新来源关联并保留旧来源、旧收藏与旧进度；各来源仍按“插件 ID + 条目 ID”独立记录。
5. 集数优先按明确的 `season + episodeNumber` 对齐；双方都没有季号时可按明确集号提出候选。
   `index`、标题相似度只能作为备选提示，匹配缺失或有歧义时让用户选集。
6. 新来源已有本地进度时优先使用该来源记录；没有时可以提议沿用旧播放位置，
   但不能默认认为拆集版、剪辑版与完整版的秒数等价，由用户确认后再定位。

为了让换源可靠，插件**应当**：

- 返回上游的原始标题，不要在 `title` 里附加"更新至 x 集""HD"之类的修饰（放进 `badge` 或 `description`）。
- 尽量提供 `year` 与 `type`。
- 用 `Episode.index` 表达列表顺序，尽量提供真实 `season`、`episodeNumber`，不要从显示文本猜造季集身份。
