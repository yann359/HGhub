# 能力与宿主 API

## 选择能力

| 场景 | 最低方法 | 可增加 |
| --- | --- | --- |
| 点播 | media.getDetail、media.resolvePlayback | search、home、catalog、分集分页 |
| 纯直播 | live.getGroups、live.listChannels、live.resolve | 节目单、图文 |
| 播放页解析 | resolver.resolve，meta.kind 为 resolver | matches、账号、资源变换 |
| 图文 | articles.list、articles.getDetail | 当前 source 仍须具备 media 或 live |
| 登录 | auth 的全部四个方法 | 多步表单、支持 `webview` 时的网页登录 |

声明命名空间就要实现其必需方法，不要用空函数伪装支持；忽略未知 params 字段。插件只返回数据，不能绘制 UI。需要输入时使用设置或 auth/ui 表单。详见[能力模块](../plugin-protocol/06-capabilities.md)。

当 `hg.env.features.includes("webview")` 为真时，`auth.getMethods` 可返回无 `form` 的网页登录方式。用户点击后，在 `auth.submit` 中调用 `hg.webview.open`，用 `doneWhen` 指定站点登录后的地址或 Cookie 名。若提供两种条件，两者都要满足。窗口完成后只返回 `finalUrl`，插件再用 `hg.http.fetch` 读取同源账号接口，按站点返回的用户 ID 和昵称生成 `UserProfile`。[参数与示例](../plugin-protocol/04-host-api.md#12-hgwebview可选)

宿主按插件和 HTTPS origin 隔离网页登录会话，并处理后续响应的 `Set-Cookie`。`hg.cookies.get/set` 只访问普通 Cookie，页面的 localStorage 留在临时视图中。重新打开账号区时，`auth.getProfile` 可通过同一接口恢复账号显示。登出时在 `finally` 调用 `hg.cookies.clear({ url })` 清理该站点的本地会话。

## 调用宿主

| 用途 | 模块 | 注意事项 |
| --- | --- | --- |
| 网络 | hg.http | 非 2xx 返回响应，插件映射业务错误 |
| 持久化 | hg.storage | 5 MiB/插件，256 KiB/值，支持 TTL |
| Cookie | hg.cookies | 每插件独立罐，不能共享其他源账号 |
| 摘要/加密 | hg.crypto | HMAC、AES、RSA 公钥加密、随机数；RSA 支持范围见下文 |
| 编解码 | hg.codec | UTF-8、Base64、Hex、legacy charset |
| HTML | hg.html | 节点仅当前调用内使用，不可作为业务返回值 |
| 设置 | hg.settings | 保存后宿主重建插件 |
| 日志 | hg.log、console | 本地日志，不写密码、Cookie、签名 URL |

工厂只构造对象并读取设置，不做网络、存储、Cookie 或 HTML 操作，也不能创建定时器。可以返回 Promise；从执行工厂开始到完成能力发现共用 5 秒预算，单段同步执行最多 1 秒。await 和微任务不会重置总预算，永不完成的 Promise 以 TIMEOUT 加载失败。方法接收 `(params, ctx)`；读取一般 15 秒，播放一般 20 秒。将 ctx.signal 传给 HTTP。调用结束后任务清理，没有自由后台执行。

同一插件并发发出的匿名 GET 由宿主按实际 URL（含 query）和有效请求头合并；不需要启用 `cache`。默认 `credentials: "include"` 但没有匹配 Cookie 时也可合并；带 Cookie 或 Authorization 的请求独立发送。每次调用保持自己的超时与 signal；一个调用取消不影响其他等待者，全部退出后停止上游请求。不同响应体上限、重定向模式或 Cookie 策略保持独立。重试、响应缓存和在途合并是不同机制，不要把某次调用的 signal 绑定到跨调用共享的 Promise；让宿主负责网络合并和取消。

API 默认每插件并发 6、同主机最多 4，读取响应体期间仍占许可。媒体使用独立队列；收到 `RATE_LIMITED` 时尊重 `retryAfterSec`，宿主按实际 429/503 响应跨插件和队列冷却该主机。

每个插件使用独立 QuickJS，调用可以在 await 之间交错。不要把当前请求参数或信号放在共享闭包变量中；需持久状态使用存储。

业务数据允许普通对象、数组、有限数字、字符串、布尔、null、Uint8Array。对象 undefined 属性丢弃；数组不能有 undefined/空位。不得返回 Date、Map、类实例、函数或环。总值最多 8 MiB、深度 32 层。

## 日志与卸载

插件日志与宿主的校验告警、运行时错误共用每分钟 240 行、最近 2000 行的限额。每行包含 details 的 UTF-8 JSON 最多 2 KiB；过长文本和 details 会截断，读取日志时可见超速丢弃数量。优先记录 requestId、错误码和字段路径，避免整段上游响应。

可选顶层 `dispose()` 仅用于同步释放内存引用或写日志，最多执行 1 秒。此时没有业务调用作用域，不能联网、持久化、读写 Cookie、操作 HTML 或创建定时器，也不会等待返回的 Promise。运行时故障可能跳过通知，持久状态应在正常业务调用中保存。

## RSA 公钥加密

`hg.crypto.rsaEncrypt` 是同步必需 API，接受 1024–8192 位 SPKI（`PUBLIC KEY`）或 PKCS#1（`RSA PUBLIC KEY`）PEM。必须指定 `pkcs1`、`oaep-sha1` 或 `oaep-sha256`，输入和输出都是 `Uint8Array`；文本先用 `hg.codec.utf8Encode`。

```js
const ciphertext = hg.crypto.rsaEncrypt(
  { publicKey: publicKeyPem, padding: "oaep-sha256" },
  hg.codec.utf8Encode(password),
);
const encoded = hg.codec.base64Encode(ciphertext);
```

填充方式由上游接口决定。OAEP 的 MGF1 使用同一摘要，label 为空；随机填充由操作系统随机源提供，相同明文的密文通常不同。单次只加密一个 RSA 块，不自动分块：2048 位密钥的上限分别为 245、214、190 字节，按 UTF-8 编码后的字节数计算。格式、类型、密钥范围或明文长度不合法时抛 `INVALID_ARGUMENT`，不提供私钥解密 API。

依赖该方法且无法降级的插件应声明 `meta.requires: [{ type: "api", name: "crypto.rsaEncrypt" }]`；旧开发版会在启用前标记不兼容。内存假宿主须注入加密适配器，算法正确性使用真实 Rust/QuickJS 宿主验证。

## 播放

先读取 `hg.env.media`，播放解析按 `params.codecs` 的顺序选择候选，不根据平台名称猜测 H.264/HEVC/AV1 支持。当前客户端在插件初始化前探测 WebView 的普通视频与 HLS 实际路径；编码列表取启用路径的保守交集，可能为空。能力 API 只用于选择候选，不保证所有 profile、分辨率和音轨组合；应保留可用备用流。没有新增协议特性，DASH 仍不可用。

返回 `streams`，MP4 使用 progressive，HLS 使用 hls。页面流为 kind:page，由解析器处理。图片使用 ImageRef，所有资源走所属插件媒体会话。

备用线路用 servers；按 serverId 只解析当前线路。有效期为 Unix 毫秒；reason:refresh 必须更新地址。不要用 index 猜跨来源同一集，也不要用上游 resumeSec 覆盖宿主本地进度。

完整签名见[宿主 API](../plugin-protocol/04-host-api.md)、[内容模型](../plugin-protocol/05-content-model.md)与[媒体传输](../plugin-protocol/09-media-transport.md)。类型表示设计契约，实际缺口见[支持状态](status.md)。
