# 01 · 插件文件与导入

[← 总览](README.md)

## 1. 插件文件

**一个插件就是一个 `.js` 文件，对应一个数据源或解析器。** 没有压缩包，没有附带文件。

- UTF-8 编码的 ES 模块，文件名任意（建议用插件 ID，如 `demo.js`）。
- 自包含：不得使用静态导入、动态 `import()` 或从外部模块再导出。作者用多文件或第三方库开发时，自行打包成单文件。
- 大小 ≤ 2 MiB。

文件有且只有两个导出：

| 导出 | 说明 |
| --- | --- |
| `export const meta` | 元信息：一个普通对象（§2） |
| `export default` | 工厂函数：接收宿主 API `hg`，返回插件对象（见 [02 §3](02-runtime.md)） |

### 1.1 最小示例

```js
export const meta = {
  id: "demo",
  name: "示例源",
  version: "1.0.0",
  protocol: 1,
  description: "一句话简介",
};

export default function (hg) {
  return {
    media: {
      async getDetail({ id }, ctx) {
        const res = await hg.http.fetch({
          url: `https://api.demo.example/series/${id}`,
          responseType: "json",
          signal: ctx.signal,
        });
        return toDetail(res.data);
      },
      async resolvePlayback({ mediaId, episodeId }, ctx) {
        /* … */
      },
    },
    search: {
      async search({ query, cursor }, ctx) {
        /* … */
      },
    },
  };
}
```

## 2. 元信息（`meta`）

| 字段 | 必需 | 说明 |
| --- | --- | --- |
| `id` | 是 | 插件的唯一标识。小写字母、数字、`.`、`_`、`-`，以字母或数字开头，≤ 64 字符 |
| `name` | 是 | 展示名称，≤ 60 字符 |
| `version` | 是 | 三段非负十进制整数版本（如 `1.2.0`），各段为安全整数、无多余前导零；不接受预发布与构建后缀 |
| `protocol` | 是 | 插件面向的协议主版本，当前为 `1` |
| `description` | 否 | 简介，≤ 500 字符 |
| `author` | 否 | 作者名 |
| `homepage` | 否 | 主页或反馈地址 |
| `updateUrl` | 否 | 检查更新时取回新文件的地址（§5.2） |
| `kind` | 否 | `source`（默认）或 `resolver`；解析器见 [06 §3](06-capabilities.md) |
| `matches` | 解析器必需 | 主机模式数组，声明该解析器能处理哪些播放页（写法见 [06 §3](06-capabilities.md)） |
| `defaultHeaders` | 否 | 媒体请求的默认请求头，优先级见 [05 §7.4](05-content-model.md) |
| `timeouts` | 否 | 各类调用的时间预算（秒），上限见 [02 §5.2](02-runtime.md) |
| `features` | 否 | 数组，声明用到的宿主可选特性（如 `["webview"]`），仅供界面提示 |
| `requires` | 否 | 必需的宿主能力数组：`{ type: "feature" \| "api", name: string }`；如 `{ type: "feature", name: "webview" }` 或 `{ type: "api", name: "html.parse" }`，缺失时保留文件但不可启用 |
| `allowInsecureHttp` | 否 | `true` 时允许插件使用 `http:` 地址（接口与媒体）；默认只允许 HTTPS |
| `linkDomains` | 否 | 主机名数组。用户粘贴这些域名下的链接时，宿主交给本插件的 `media.matchUrl` 处理（见 [06 §2](06-capabilities.md)） |
| `mature` | 否 | `true` 表示含成人内容；宿主默认隐藏这类插件，用户需在设置中开启 |
| `settings` | 否 | 用户可调的设置项（§2.1） |

规则：

- `meta` **必须**是字面量可求值的普通对象，求值不得依赖 `hg` 或产生副作用——宿主会在不调用工厂函数的
  情况下读取它。
- `id` 是插件的身份：片库、存储、设置都以它为键。改 `id` 等于换了一个插件。
- 宿主**必须**忽略不认识的字段，以便旧宿主读取新版插件。
- `protocol` 高于宿主支持的主版本时，宿主拒绝导入并提示需要更新应用。

插件实现了哪些能力**不需要**在 `meta` 里声明：宿主在首次启用时执行工厂、校验能力方法并缓存结果，
再按 [02 §6](02-runtime.md) 完成默认线路初始化。初始化完成前不向首页、搜索等业务入口发布该插件。
缓存与插件文件版本及内容修订绑定；同版本覆盖也必须使缓存失效。`requires` 的兼容行为见 [07 §1](07-ecosystem.md)。

### 2.1 请求头与时间预算

```js
export const meta = {
  id: "demo", name: "示例源", version: "1.0.0", protocol: 1,
  // 90% 的图片与流都要同一个 Referer，逐条写是噪音，写漏一条就是 403
  defaultHeaders: { Referer: "https://www.example.com/", "User-Agent": "Mozilla/5.0 …" },
  // 上游慢时可以放宽；宿主会夹在上限内（02 §5.2）
  timeouts: { read: 25, playback: 45 },
};
```

- `defaultHeaders` 只作用于**媒体请求**（图片、播放列表、分片、字幕），不影响 `hg.http` 的接口请求。
  优先级为：条目或流自身的 `headers` > `defaultHeaders` > 宿主默认值（见 [05 §7.4](05-content-model.md)）。
- `timeouts` 的键为 `read`、`playback`、`login`、`transform`，单位秒。

### 2.2 设置项

```js
settings: [
  { key: "quality", type: "select", label: "默认清晰度", default: "auto",
    options: [{ value: "auto", label: "自动" }, { value: "1080", label: "1080P" }] },
  { key: "preferHevc", type: "switch", label: "优先 HEVC", default: false },
  { key: "apiKey", type: "password", label: "接口密钥" },
]
```

每项包含 `key`、`type`、`label`，以及可选的 `description`、`default`。

| `type` | 值类型 | 额外字段 |
| --- | --- | --- |
| `text` | 字符串 | `placeholder` |
| `password` | 字符串（界面掩码显示） | `placeholder` |
| `switch` | 布尔 | — |
| `number` | 数字 | `min`、`max` |
| `select` | 字符串 | `options: [{ value, label }]` |

宿主负责渲染设置表单并保存用户的值；插件通过 `hg.settings` 读取（见 [04](04-host-api.md)）。

## 3. 导入方式

| 方式 | 说明 |
| --- | --- |
| 本地 `.js` 文件 | 可一次选择多个 |
| 本地 `manifest.json` | 按其中的列表批量导入（§4） |
| URL | 下载后按内容识别单个 JS 插件或 JSON 插件列表，支持无扩展名的下载接口 |
| 粘贴代码 | 以 UTF-8 源码导入单个插件；无来源 URL 时需 `updateUrl` 才能检查更新 |

URL 路径扩展名与 `Content-Type` 仅作提示，不作为拒绝依据。宿主先尝试解析为 JSON 插件列表，
否则按 JS 模块读取元信息；识别失败时报告具体错误，不把 HTML 错误页保存为插件。
相对地址以重定向后的最终列表 URL 为基准；下载仍受宿主网络规则与对应文件大小限制约束。

导入单个插件的流程：

```text
取得文件内容 → 读取 meta 并检查必需字段与 protocol → 写入插件目录并记录来源 → 完成
```

- 导入只读取 `meta`，不调用工厂函数。
- “读取”采用受时间与内存预算约束的临时运行时求值模块，不是静态提取；模块顶层不得执行外部 I/O。
  顶层求值与元信息检查共用 5 秒预算。顶层 `await`、异常、死循环均受此预算约束。
- 导入成功后默认启用并触发初始化；用户选择仅安装时保持停用。初始化失败保留文件，显示原因并允许重试。
- 批量导入时，单个插件失败不影响其余插件；结束后汇总成功与失败的清单及原因。
- 宿主记录每个插件的**来源**（本地路径或 URL），用于之后的更新检查。

## 4. 插件列表（`manifest.json`）

```json
{
  "name": "某某的插件集",
  "description": "可选的说明",
  "plugins": [
    {
      "id": "demo",
      "name": "示例源",
      "version": "1.0.0",
      "description": "一句话简介",
      "url": "demo.js"
    },
    {
      "id": "another",
      "name": "另一个源",
      "version": "0.3.1",
      "url": "https://cdn.example.com/plugins/another.js"
    }
  ]
}
```

| 字段 | 必需 | 说明 |
| --- | --- | --- |
| `name` | 否 | 列表名称 |
| `description` | 否 | 列表说明 |
| `plugins` | 是 | 插件条目数组 |
| `plugins[].url` | 是 | 插件文件地址。可以是绝对 URL，或相对于 `manifest.json` 自身位置的相对路径 |
| `plugins[].id` | 是 | 与插件文件内 `meta.id` 一致 |
| `plugins[].name` | 是 | 展示名称 |
| `plugins[].version` | 是 | 与插件文件内 `meta.version` 一致 |
| `plugins[].description` | 否 | 简介 |

规则：

- 列表里的 `id`、`name`、`version`、`description` 只用于**在下载之前**向用户展示和比对版本；
  导入后一律以插件文件内的 `meta` 为准。但下载文件的 `id`、`version` 与列表不一致时，本条导入失败，
  不得作为其他身份安装或覆盖；这通常表示列表与文件发布不同步。
- 相对路径的解析基准是 `manifest.json` 所在的目录——本地导入时是该文件的所在文件夹，
  URL 导入时是该 URL。
- Android 文档提供者的 `content://` 地址不是文件系统路径。相对条目需要用户授权列表所在目录；
  宿主按提供者的文件名逐层查询，不拼接不透明文档 ID。只选择列表文件不授予兄弟文件访问权。
  相对路径不得越出授权目录，也不得借绝对路径、编码分隔符、查询或片段改变文件定位；
  绝对 HTTP(S) 条目仍按远程下载处理。目录授权属于宿主导入流程，不向插件开放文件访问 API。
- 用户可以在列表界面勾选要导入的插件，默认全选。
- 宿主**必须**忽略不认识的字段。
- 条目数 ≤ 500；文件 ≤ 1 MiB。

### 4.1 订阅

从 URL 导入插件列表后，宿主**可以**把该地址保存为"订阅"：之后用户可以一键刷新，
看到列表中新增的插件与有更新的插件。这只是对 §3、§5 的界面包装，不引入新的协议概念。

## 5. 覆盖与更新

### 5.1 同 ID 覆盖

导入一个 `id` 已存在的插件时：

| 新文件的版本 | 宿主行为 |
| --- | --- |
| 更高 | 直接更新 |
| 相同 | 覆盖（方便作者调试） |
| 更低 | 提示这是降级，由用户确认 |

覆盖保留该插件的存储、设置与片库记录。新版本的 `settings` 里不再存在的 `key`，对应的值被清除；
保留键的值类型或选项已不合法时回退为新默认值。插件更新原子替换文件、使能力缓存失效并重新初始化；
初始化失败时提供恢复上一份可用文件的操作，不回滚插件已经执行的上游操作或存储写入。
使用该插件的活跃播放会话在替换前停止并提示重新播放，避免旧资源混用新代码；检查更新不影响播放。

### 5.2 检查更新

宿主按以下顺序确定一个插件的更新地址：`meta.updateUrl` → 导入时的来源 URL → 无（本地导入且未声明，不检查）。

- 声明 `meta.updateUrl` 时直接取回该 JS 文件；否则，来源是插件列表时刷新列表并比对条目版本，
  有更高版本再下载；来源是单个 JS 地址时直接取回文件并比对版本。
- 默认由用户手动触发。宿主**可以**提供"启动时自动检查"的开关，自动检查同一地址的间隔不短于 24 小时。
- 发现新版本后是否自动安装，由用户在设置里决定；默认只提示。自动更新只接受同 ID 的更高版本，
  不自动覆盖同版本或降级；正在播放时延后安装。24 小时间隔只约束自动检查，用户手动检查不受此间隔限制。

版本比较按数字段逐段进行（`1.10.0` 高于 `1.9.0`）；不合法的版本号在导入或检查更新时明确报错，不猜测为 0。

### 5.3 移除

移除插件时清除它的文件、存储、Cookie、缓存与设置。
宿主本地片库中引用该插件的收藏与历史**应当**保留并标记为"来源不可用"，以便重新导入后恢复。
