# 04 · 宿主 API

[← 总览](README.md)

宿主 API 是插件与外界交互的**唯一**途径。它以对象 `hg` 的形式传给工厂函数（见 [02 §3](02-runtime.md)）。
所有插件看到的是同一套 `hg`。

## 1. 模块一览

| 模块 | 用途 | 同步/异步 |
| --- | --- | --- |
| [`hg.env`](#3-hgenv) | 运行环境信息 | 同步只读 |
| [`hg.endpoint`](#4-hgendpoint) | 当前站点线路 | 同步只读 |
| [`hg.http`](#2-hghttp) | 网络请求 | 异步 |
| [`hg.cookies`](#5-hgcookies) | Cookie 罐 | 异步 |
| [`hg.storage`](#6-hgstorage) | 持久键值存储 | 异步 |
| [`hg.crypto`](#7-hgcrypto) | 密码学原语 | 同步 |
| [`hg.codec`](#8-hgcodec) | 编码转换 | 同步 |
| [`hg.html`](#9-hghtml) | HTML 解析与查询 | 同步 |
| [`hg.settings`](#10-hgsettings) | 读取用户设置 | 同步只读 |
| [`hg.log`](#11-hglog) | 日志 | 同步 |
| [`hg.webview`](#12-hgwebview可选) | 请用户在网页中完成操作 | 异步 |
| `hg.PluginError` | 错误类型 | — |

通用规则：

- 异步接口失败时抛出 `hg.PluginError`（错误码见 [02 §8](02-runtime.md)）。
- 业务数据使用可传递值；`signal`、节点句柄、错误等运行时对象的例外见 [02 §4](02-runtime.md)。
- 异步操作自动绑定当前调用作用域，无需插件传 signal 才能随调用取消；显式 signal 只增加提前取消条件。
- 工厂函数执行期间，除 `hg.env`、`hg.endpoint`、`hg.settings`、`hg.log`、`hg.crypto`、`hg.codec`
  之外的模块不可用。

## 2. `hg.http`

```ts
hg.http.fetch(request: HttpRequest): Promise<HttpResponse>
```

### 2.1 请求

```ts
interface HttpRequest {
  url: string;                       // 绝对 https 地址
  method?: "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE";   // 默认 GET
  headers?: Record<string, string>;
  query?: Record<string, string | number | boolean | undefined>;   // 追加到 url
  body?: string | Uint8Array | { json: Json } | { form: Record<string, string> };
  responseType?: "text" | "json" | "bytes";                         // 默认 text
  charset?: string;                  // text/json 的解码字符集
  timeoutMs?: number;                // 默认 15000，上限 30000
  redirect?: "follow" | "manual";    // 默认 follow
  maxBytes?: number;                 // 响应体上限，默认 2 MiB，最大 8 MiB
  cache?: { ttlSec: number; staleIfErrorSec?: number };
  retry?: 0 | 1 | 2;                 // 默认 0
  credentials?: "include" | "omit";  // 是否使用 Cookie 罐，默认 include
  signal?: AbortSignal;
}
```

| 字段 | 规则 |
| --- | --- |
| `url` | 长度 ≤ 8 KiB；目标受 [03 §2](03-host-rules.md) 的网络规则约束，被拦截时抛 `NETWORK` |
| `headers` | 总大小 ≤ 16 KiB；由传输层决定的头会被丢弃（[03 §2](03-host-rules.md)）。未指定 `User-Agent` 时为 `HGhub/<版本>` |
| `query` | 值为 `undefined` 的键被忽略；按表单规则编码后追加 |
| `body` | ≤ 1 MiB。`{ json }` 自动设置 `Content-Type: application/json`；`{ form }` 自动设置 `application/x-www-form-urlencoded`；显式给出的 `Content-Type` 优先。`GET`、`HEAD` 不得带请求体 |
| `charset` | 省略时取响应头中的字符集，回退 UTF-8；其他字符集需特性 `charset.legacy` |
| `maxBytes` | 响应体超出即中止并抛 `UPSTREAM_ERROR` |

### 2.2 响应

```ts
interface HttpResponse<T> {
  status: number;
  ok: boolean;                       // 200–299
  url: string;                       // 最终地址
  headers: Record<string, string>;   // 键小写；多值以 ", " 连接
  setCookies: string[];              // 对插件可见的原始 Set-Cookie，逐条
  data: T;                           // 按 responseType：string / 解析后的 JSON / Uint8Array
  fromCache: boolean;
}
```

- **非 2xx 不抛错。** 插件根据 `status` 自行映射为合适的 `PluginError`。
- `responseType: "json"` 而响应体不是合法 JSON 时抛 `UPSTREAM_CHANGED`。
- `gzip`、`br` 压缩由宿主处理。
- 目标 origin 有宿主管理的网页登录会话时，响应中的全部 `Set-Cookie` 由宿主处理，`headers` 省略 `set-cookie`，`setCookies` 为空。`credentials: "omit"` 跳过自动发送及写入，同样隐藏该站点的 `Set-Cookie`。有 `Set-Cookie` 的响应即使隐藏了该头也不能缓存。

### 2.3 重定向

| 请求 | 行为 |
| --- | --- |
| `redirect: "manual"` | 不跟随；3xx 响应原样返回，`headers.location` 可读 |
| 带请求体（`POST` 等） | 恒按 `manual` 处理，避免重复提交 |
| 其他 | 跟随，至多 5 跳；每跳重新过网络规则 |

重定向跨 origin 时移除显式 Cookie 与 Authorization，不重新向跨源目标附加原始凭据头；
自动 Cookie 每跳按目标 URL 重新匹配所属插件的 Cookie 罐。

### 2.4 缓存

`cache` 让宿主代为缓存响应，减少对上游的重复请求。

- 只对方法为 `GET`、未带 `Authorization` 头、且未使用 Cookie 的请求生效；不满足时静默忽略。
- 缓存键包含插件、入口请求地址（含查询串）及全部有效请求头，并按 `Vary` 区分响应；
  `Vary: *`、`Cache-Control: no-store` 或设置 Cookie 的响应不缓存。重定向后的目标受每跳网络规则约束，
  不得只以最终地址合并入口语义不同的请求。
- 只缓存 2xx；`ttlSec` 上限 86400。
- `staleIfErrorSec`：新请求因 `NETWORK`、`TIMEOUT` 或 5xx 失败时，可返回过期不超过该时长的旧响应
  （上限 7 天），此时 `fromCache` 为 `true`。
- HTTP 缓存使用独立的可淘汰配额，不占 `hg.storage` 的 5 MiB；媒体缓存也独立管理。
  缓存不足只影响命中率，不得淘汰持久化数据或导致存储写入失败；用户“清除缓存”不清除存储与登录态。

### 2.5 重试

`retry` 只对 `GET`、`HEAD` 生效，在 `NETWORK`、`TIMEOUT` 或 5xx 时自动重试，间隔 400 毫秒、1200 毫秒。
重试同样消耗限速配额；处于冷却期的主机不重试。

### 2.6 可能抛出的错误

| 错误码 | 情形 |
| --- | --- |
| `NETWORK` | DNS、连接、TLS 失败，或目标被网络规则拦截 |
| `RATE_LIMITED` | 排队超时，或主机处于冷却期（带 `retryAfterSec`） |
| `TIMEOUT` | 超过 `timeoutMs` 或调用的截止时间 |
| `CANCELLED` | `signal` 被触发 |
| `UPSTREAM_ERROR` | 响应体超过 `maxBytes` |
| `UPSTREAM_CHANGED` | `responseType: "json"` 解析失败 |
| `INVALID_ARGUMENT` | 地址不合法、请求体过大、`GET` 带请求体等 |

## 3. `hg.env`

```ts
interface HostEnv {
  protocol: string;                    // 宿主实现的协议版本，如 "1.0.0"
  app: { name: string; version: string };
  plugin: { id: string; version: string };
  platform: "windows" | "macos" | "linux" | "android" | "ios";
  locale: string;                      // 界面语言，BCP 47
  features: readonly string[];         // 宿主支持的可选特性
  media: {
    codecs: readonly ("h264" | "hevc" | "av1")[];   // 当前播放路径实际可用，按偏好排序
    formats: readonly ("hls" | "dash" | "progressive")[];
  };
}
```

- 全部为插件加载时的快照。界面语言等变化后，宿主重载插件。
- 插件**应当**用 `features` 与 `media` 做能力探测，而不是根据 `platform` 或 `app.version` 推断。
- 媒体能力以当前播放路径的能力 API 探测为依据；编码名是编码家族，不保证该家族所有 profile、
  分辨率、码率或音视频组合均能播放，也不等同于实际样本解码验收。未探测或没有可靠支持结果时允许空列表，
  不得默认宣称 H.264 或按操作系统名称猜测。HGhub 当前探测方式与保守范围见 [实现说明](IMPLEMENTATION.md#播放能力探测2026-11-16)。
- 点播、直播及解析器收到的 `params.codecs` 必须与该插件加载快照的 `hg.env.media.codecs` 一致，
  包括正常播放、预加载和刷新。宿主探测结果变化时重建运行时，不能原地修改已有只读快照。
- 需要"设备号"的上游，请自行生成随机值并存入 `hg.storage`。

### 3.1 可选特性

| 特性名 | 含义 |
| --- | --- |
| `webview` | 支持 `hg.webview`（[§12](#12-hgwebview可选)） |
| `charset.legacy` | `TextDecoder`、`hg.codec.decodeText`、`hg.http` 的 `charset` 支持 GBK / GB18030 / Big5 / Shift_JIS / EUC-KR |
| `transform.segment` | 支持对视频分片做资源变换（见 [05](05-content-model.md)） |
| `format.dash` | 当前播放器与宿主 MPD/分段代理均支持 MPEG-DASH（见 09） |

插件在 `meta.features`（数组）中声明用到的特性，仅供界面提示；运行时**应当**自行探测并降级。

## 4. `hg.endpoint`

```ts
interface EndpointApi {
  readonly current: {
    id: string;
    label?: string;
    url: string;           // 纯 origin，如 "https://api.example.com"
  } | null;
  url(path: string): string;   // 把以单个 "/" 开头的路径拼到当前线路上；拒绝 "//host" 等跨 origin 形式
}
```

- 适用于上游有多个入口域名的情况。插件实现 `endpoints` 能力（见 [06](06-capabilities.md)）后，
  宿主在插件详情页展示线路列表并保存用户的选择，插件通过这里读取。
- 插件没有实现该能力或尚无默认选择时 `current` 为 `null`，`url()` 抛 `UNSUPPORTED`。
- 首次启用时先获取线路列表、保存默认选择，必要时重建上下文，完整流程见 [02 §6](02-runtime.md)。
- 切换线路会重启插件上下文；普通存储不随之切换，凭据是否绑定 origin 由插件决定。

## 5. `hg.cookies`

```ts
hg.cookies.get(params: { url: string }): Promise<Cookie[]>
hg.cookies.set(params: { url: string; cookie: Cookie }): Promise<void>
hg.cookies.clear(params?: { url?: string }): Promise<void>

interface Cookie {
  name: string; value: string;
  domain?: string; path?: string;
  hostOnly?: boolean;      // get 返回匹配属性；set 省略 domain 时为 true
  expiresAt?: number;      // Unix 毫秒；省略为会话 Cookie
  secure?: boolean; httpOnly?: boolean;
}
```

- 普通 Cookie 罐按**插件**隔离并持久保存；罐内按请求 URL 的 domain、host-only、path、secure、expires 等正常 Cookie 规则匹配，
  不因当前站点线路变化而整体切换。同一服务跨镜像能否登录由上游 Cookie 域与插件登录逻辑决定。
- `get({ url })` 返回普通罐中匹配该 URL 的 Cookie；`set` 的 domain 必须能由给定 URL 设置；
  `clear({ url })` 删除匹配该 URL 的 Cookie，不传 url 则清空该插件全部 Cookie。
- `set` 省略 domain 时绑定 URL 的主机；给出 domain 时默认非 host-only，可显式设为 true 限定该主机。
- `hg.http.fetch` 自动保存响应的 `Set-Cookie` 并在后续请求中附加，除非 `credentials: "omit"`。
- 会话 Cookie 在插件卸载后**保留**（插件随时可能被卸载，不能以此模拟浏览器会话结束）；
  插件登出或用户清除数据时清空。

### 5.1 宿主管理的网页登录会话

- 一次性视图成功移交的会话按 **插件 ID + 起始 HTTPS origin** 保存。origin 包括主机和有效端口。完成页须回到起始 origin，第三方身份提供方的 Cookie 只留在临时视图。
- `credentials: "include"`（默认）以及该插件的媒体请求，在该 origin 有网页登录会话时使用其专用罐，仍按 Cookie 自身的 domain、host-only、path、secure 和过期属性匹配。每次重定向重新选择目标的罐。显式 `Cookie` 请求头沿用既有的覆盖规则。
- `hg.cookies.get/set` 只访问普通罐。网页登录 Cookie 和后续同源 `Set-Cookie` 都留在宿主管理的罐内，插件用同源账号接口读取用户资料。源和解析器具有独立归属。
- 对同一 origin 再次登录成功，原子替换此前的网页登录会话。失败或取消保留旧会话。`hg.cookies.clear({ url })` 清除该 origin 的整个网页登录会话（包括其他路径），并按普通规则清除匹配 URL 的普通 Cookie。无参数清除该插件两类罐。注销或换号前发出的请求，其迟到响应不能恢复或修改旧会话。

## 6. `hg.storage`

```ts
hg.storage.get<T>(key: string): Promise<T | undefined>
hg.storage.set(key: string, value: TransferValue, options?: { ttlSec?: number }): Promise<void>
hg.storage.delete(key: string): Promise<void>
hg.storage.keys(prefix?: string): Promise<string[]>
hg.storage.clear(): Promise<void>
```

- 键：1–256 字节的 UTF-8 字符串。值：任意可传递值（`Uint8Array` 也支持），单个 ≤ 256 KiB，总计 ≤ 5 MiB。
- `TransferValue` 指 [02 §4](02-runtime.md) 的可传递值。命名空间只按**插件 ID**划分，
  切换站点线路不切换存储；插件内部迁移、设备随机标识及跨线路配置可持续使用。
- `ttlSec`：到期后 `get` 返回 `undefined`（过期条目由宿主回收），适合放可重建的缓存。
- 写入是原子的：`set` 返回后数据已持久化，插件随后被卸载不会丢失。
- 存储为明文（应用数据目录内）。令牌是否跨镜像共享由插件决定；需要绑定 origin 时，
  可使用 `auth:<origin>` 一类键前缀，宿主不强制替插件决定认证范围。
- 持久数据不自动淘汰；超出配额时 `set` 抛 `STORAGE_FULL`，此前的值保持完整。
  并发的读后写不是事务；插件需要自行协调同一键的复合更新。
- 移除插件时全部清除。

## 7. `hg.crypto`

原生实现的同步原语。字符串入参按 UTF-8 编码。

```ts
type Hash = "md5" | "sha1" | "sha256" | "sha512";

hg.crypto.digest(algorithm: Hash, data: Uint8Array | string): Uint8Array
hg.crypto.hmac(algorithm: Hash, key: Uint8Array | string, data: Uint8Array | string): Uint8Array
hg.crypto.aesEncrypt(params: AesParams, data: Uint8Array): Uint8Array
hg.crypto.aesDecrypt(params: AesParams, data: Uint8Array): Uint8Array
hg.crypto.rsaEncrypt(params: RsaParams, data: Uint8Array): Uint8Array
hg.crypto.randomBytes(length: number): Uint8Array      // ≤ 65536
hg.crypto.randomUUID(): string

interface AesParams {
  mode: "cbc" | "ecb" | "ctr" | "gcm";
  key: Uint8Array;                  // 16 / 24 / 32 字节
  iv?: Uint8Array;                  // cbc、ctr：16 字节；gcm：12 字节；ecb：省略
  padding?: "pkcs7" | "none";       // 仅 cbc、ecb，默认 pkcs7
  aad?: Uint8Array;                 // 仅 gcm
}

interface RsaParams {
  publicKey: string;                // PEM（SPKI 或 PKCS#1）
  padding: "pkcs1" | "oaep-sha1" | "oaep-sha256";
}
```

- GCM 的 16 字节认证标签附在密文末尾；校验失败抛 `INVALID_ARGUMENT`。
- `rsaEncrypt` 只做公钥加密（部分上游要求用公钥加密登录密码）。公钥模数为 1024–8192 位；
  PEM 标签为 `PUBLIC KEY`（SPKI）或 `RSA PUBLIC KEY`（PKCS#1），不接受私钥。
- RSA 的 `pkcs1` 表示 RSAES-PKCS1-v1_5；OAEP 使用所选摘要及相同摘要的 MGF1，label 为空。
  由系统安全随机源提供填充随机性，返回长度为模数字节数的密文，不自动分块。
- 设模数字节数为 k，RSA 单次明文上限：`pkcs1` 为 k−11，`oaep-sha1` 为 k−42，
  `oaep-sha256` 为 k−66。`data` 必须是 `Uint8Array`；文本先 UTF-8 编码，再按字节计算长度。
  无效 PEM/公钥、填充、类型或超长明文抛 `INVALID_ARGUMENT`；随机源失败抛 `INTERNAL`。
- 上表之外的算法由插件自行打包纯 JS 实现。

## 8. `hg.codec`

```ts
hg.codec.base64Encode(data: Uint8Array | string, options?: { urlSafe?: boolean; pad?: boolean }): string
hg.codec.base64Decode(text: string): Uint8Array      // 同时接受标准与 URL 安全字母表
hg.codec.hexEncode(data: Uint8Array | string): string
hg.codec.hexDecode(text: string): Uint8Array
hg.codec.utf8Encode(text: string): Uint8Array
hg.codec.utf8Decode(data: Uint8Array): string
hg.codec.decodeText(data: Uint8Array, charset: string): string
```

输入不合法时抛 `INVALID_ARGUMENT`。`decodeText` 对 UTF-8 之外的字符集需特性 `charset.legacy`。

## 9. `hg.html`

不少上游只有页面可解析。宿主提供原生的 HTML 解析与 CSS 选择器查询，
避免每个插件各自打包体积庞大的解析库。

```ts
hg.html.parse(html: string): HtmlNode

interface HtmlNode {
  readonly tag: string;                       // 小写标签名；文档根为 "#document"
  select(selector: string): HtmlNode[];
  first(selector: string): HtmlNode | null;
  text(): string;                             // 后代文本拼接并折叠空白
  ownText(): string;                          // 仅直接子文本节点
  attr(name: string): string | null;
  attrs(): Record<string, string>;
  html(): string;                             // 内部 HTML
  outerHtml(): string;
  parent(): HtmlNode | null;
  children(): HtmlNode[];
}
```

- 按 HTML5 规则解析；**不执行脚本、不加载任何子资源**。
- 选择器支持 CSS Selectors Level 3，外加 `:has()`；不合法时抛 `INVALID_ARGUMENT`。
- 输入 ≤ 8 MiB。
- 节点句柄是宿主对象的引用：只能在**创建它的那次调用内**使用，调用结束后失效；
  不可传给其他 `hg.*` 接口，不可作为返回值。
- 需要执行页面脚本才能取得的数据，由插件自行还原其算法；协议不提供页面内脚本执行。

## 10. `hg.settings`

```ts
hg.settings.get<T>(key: string): T | undefined
hg.settings.getAll(): Record<string, Json>
```

- 返回用户为该插件设置的值；未设置时返回 `meta.settings` 里的 `default`；未定义的键返回 `undefined`。
- 设置是只读的：只有用户能通过宿主界面修改。
- 用户修改设置后，宿主统一取消该插件在途调用并重新初始化，不提供 `onSettingsChanged` 钩子。
  设置界面可以合并一次表单提交中的多项修改，避免逐字段反复重载。

## 11. `hg.log`

```ts
hg.log.debug(...args: unknown[]): void
hg.log.info(...args: unknown[]): void
hg.log.warn(...args: unknown[]): void
hg.log.error(...args: unknown[]): void
```

参数被格式化为一行文本；对象以 JSON 序列化，`Uint8Array` 只记录长度。
限额与可见性见 [02 §10](02-runtime.md)。超出速率的日志被丢弃并计数。

## 12. `hg.webview`（可选）

需要特性 `webview`。本节定义带完成条件的网页登录。

```ts
hg.webview.open(params: {
  url: string;                 // 起始页
  title?: string;              // 外框标题，≤ 60 字符
  doneWhen: {
    urlPrefix?: string;        // 导航到以此为前缀的地址时视为完成
    cookieNames?: string[];    // 这些 Cookie 全部出现时视为完成
  };
  timeoutSec?: number;         // 默认 300，上限 600
}): Promise<{ finalUrl: string }>
```

- 宿主在前台展示一个带外框的网页视图（显示当前域名与关闭按钮），用户自行完成页面上的操作。
- 网页视图使用一次性、独立的数据目录，与应用界面不同源，且不能访问 `hg`。
- 网页由平台原生网络栈加载，能力声明不等于所有请求都经过 hg.http；网络策略作用范围见 [03 §2](03-host-rules.md)。
- `doneWhen` 至少给出一个条件；同时给出 URL 与 Cookie 条件时必须全部满足。
  URL 条件匹配相同的 HTTPS origin 和完整路径段前缀。例如 `/account` 匹配 `/account` 和 `/account/me`，不匹配 `/accounting`。查询参数和片段不参与路径匹配。
  只有起始导航后的主框架导航或 Cookie 状态变化才能触发完成，不能因初始 URL 已匹配就立即关闭。
  Cookie 条件针对起始 origin 的当前主框架地址判定。完成时只提交该地址匹配的 Cookie，并按照 [§5.1](#51-宿主管理的网页登录会话) 在宿主保存。`open` 返回 `finalUrl`，插件随后使用同源账号接口确认登录态。页面 localStorage 不进入返回值。
- 宿主确认 `doneWhen` 满足后完成登录。用户在完成前关闭视图时抛 `CANCELLED`，超时抛 `TIMEOUT`。
  打开期间暂停本次调用的剩余预算，`ctx.deadline` 动态为 `null`，关闭后按剩余预算恢复。用户离开、取消与应用挂起仍可结束该调用并关闭视图。
- 同一插件同时至多一个网页视图。HGhub 只允许用户提交认证表单时从 `auth.submit` 打开，其他入口抛 `UNSUPPORTED`。窗口已打开时，同一插件的第二次打开请求抛 `RATE_LIMITED`。

用户点击插件的网页登录方式后，`auth.submit` 可采用以下流程。

```js
await hg.webview.open({ url: hg.endpoint.url("/login"), title: "登录账号",
  doneWhen: { urlPrefix: hg.endpoint.url("/account"), cookieNames: ["session"] } });
const user = await api.getProfile(ctx); // 通过同源账号接口查询
if (!user) throw new hg.PluginError("AUTH_INVALID", "站点尚未确认登录");
return { status: "ok", user };
```

## 13. `hg.PluginError`

```ts
new hg.PluginError(code: ErrorCode, message?: string, options?: {
  retryAfterSec?: number;
  retryable?: boolean;
  details?: Json;
  cause?: unknown;
})
```

`hg.PluginError` 是 `Error` 的子类，实例带有 `code`、`retryable`、`retryAfterSec`、`details`。
语义与错误码见 [02 §8](02-runtime.md)。
