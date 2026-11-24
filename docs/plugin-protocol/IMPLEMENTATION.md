# 实现进度与验证范围

本页跟踪 `main` 分支的开发版本。应用清单版本为 **0.0.5**，运行时协议为 `1.0.0-dev`。HPP 1.0 仍处于草案阶段，各项能力的实现和验证范围列在下文。

本阶段以 Windows 为主。Android 已验证单文件导入、单部真实 MP4 播放和网页登录。

这是保留既有 HGhub 前端风格的全新插件播放器。不读取、不迁移旧账号、收藏或历史；
应用使用独立标识 `com.hghub.player`，首次启动空片库。旧站点 API、线路、登录与媒体解密代码已移除。

## 已落地

2026-11-20 字幕补验：真实 QuickJS 与 loopback 代理已验证 SRT 转 UTF-8 VTT、轨道元信息、转换后 Range/HEAD、坏字幕隔离及会话释放。真实来源单条 SRT 经代理返回 200 / text/vtt，1778 条 cue、112299 字节；这不替代设备上的字幕显示与切换验收。编码按 BOM、响应 charset、UTF-8 顺序处理，ASS 忽略；多条默认字幕优先当前解析器的首条，否则源的首条。

| 层 | 实现 |
| --- | --- |
| 独立宿主 | `src-tauri/crates/hpp`，不依赖原短剧平台 API、凭据、媒体解密逻辑 |
| 插件管理 | 原生多文件选择、粘贴、URL 和本地/远程 manifest 勾选导入；简介与来源列表名称展示；元信息及版本校验，降级显式选择，启停、重试、移除、索引恢复 |
| 插件更新 | 手动检查和勾选安装；updateUrl 优先、数值版本比较、来源记录持久化；预览不替换现有插件，修订变化拒绝过期更新；失败后恢复上一可用文件，保留设置与存储 |
| 初始化 | 模块顶层求值与元信息检查共用 5 秒预算；工厂另有 5 秒总预算；两者支持 Promise/微任务、每段同步 1 秒限制及挂起超时；取消初始化停止线程；首次以空 endpoint 获取线路，选定默认线路后重建；设置和站点线路变更重建 |
| 执行 | 每插件独立 QuickJS runtime、线程、64 MiB 堆；8 个并发调用、40 个总排队槽位、调用预算与同步中断 |
| 生命周期 | Promise/await 上下文归属、调用取消、遗漏 signal 的 HTTP 取消、定时器清理、空闲卸载、连续故障停止及显式重试；可运行上下文卸载时尽力同步调用 dispose，最多 1 秒，不等待 Promise，故障运行时跳过 |
| 日志 | 插件日志、校验告警和运行时错误共用每分钟 240 行/最近 2000 行/每行 2 KiB UTF-8 JSON 上限；超速丢弃数量在读取日志时可见；过长 details 截断，不绕过文本限额 |
| 数据桥 | 全量带标签的可传递值；支持 Uint8Array，不与普通对象冲突；深度、字节预算与结果校验 |
| 持久化 | 插件 ID 隔离的键值存储，二进制、TTL、原子落盘、容量上限；Cookie 独立罐与 domain/path 匹配 |
| HTTP | DNS/重定向检查，复用应用现有网络策略；同组 DNS 有允许的 IPv4 时忽略受限 IPv6，不连接受限地址；取消、请求/响应上限、字符集、限速、Retry-After 冷却；GET/HEAD 重试和有界匿名 GET 响应缓存；按插件、源站及代理/安全策略复用连接池，定期重新验证 DNS |
| 网络调度 | 每插件 API 并发 6、同主机 4，许可覆盖响应体；同插件匿名 GET 按每跳实际地址与有效头合并，独立超时/取消，全部等待者退出后停止上游；媒体不占 API 队列和速率额度，主机冷却跨插件/队列共享 |
| 原生工具 | URL、URLSearchParams、编解码、摘要/HMAC、AES CBC/ECB/CTR/GCM、RSA 公钥加密（SPKI/PKCS#1，PKCS1/OAEP-SHA1/OAEP-SHA256，1024–8192 位）、随机数，原生 HTML/CSS 选择器 |
| 媒体 | 随机端口 loopback HTTP 服务，progressive 流式代理与真实 Range/HEAD/206/416，HLS 主/子清单及 URI 属性改写 |
| 资源归属 | 解析器匹配/选择；解析器独立 Cookie、请求头和变换；源与解析器字幕保留各自提供者；同一解析动作共享预算 |
| SRT 字幕 | 按轨道 format 读取并转为 UTF-8 VTT，不依赖扩展名或上游 MIME；保留语言、标签、默认项及提供者请求环境；输入/输出各限 8 MiB，Range/HEAD 对转换后内容计算 |
| 外部流 | 源/解析器返回 external 后由宿主生成确认目标；点播和共用播放器显示地址，点击确认才交系统打开；取消、过期及系统打开失败可见，不传头/Cookie/变换，不建立代理会话；预加载不触发外部打开 |
| 字节变换 | 有界图片/清单/分片/密钥的原生 AES-CBC、plugin transformResource；对输出计算局部范围 |
| 会话 | 图片批次和播放会话；释放取消传输，依赖插件停用/移除/更新/设置变更时释放；修订校验防止旧会话使用新环境 |
| Windows 页面 | 经典沉浸播放器外壳；首页直接播放，片源范围在设置选择；推荐列表保留来源/分区排序与显隐；分区完整列表导航；分类树成功结果按插件修订缓存、筛选与排序；推荐/分类/搜索/分区滚动分页、失败保留内容并重试原游标、ID 去重与重复游标终止；单源/多源/综合搜索、来源选择；插件管理、设置、线路、日志 |
| 站点线路 | 手动刷新、可达性探测与切换；刷新后保留仍存在的线路，否则选择第一项并重建；探测不携带插件默认头或 Cookie，不自动切换 |
| 本地片库 | Rust 原子落盘，收藏/追更/历史按插件与作品隔离；进度另含播放列表与真实分集 ID；离开/暂停/结束保存、重新选集续播；插件移除后记录保留 |
| 详情与播放 | 按季分组，显式 season/episodeNumber 唯一时定位候选，不用 index 猜测；分集分页、刷新、HLS/普通视频播放；同版本覆盖修订使页面与会话失效 |
| 跨来源换源 | 详情页与播放器共用换源入口；并行搜索其他可用来源，标题/年份/类型排序、独立失败重试；候选详情、分集分页与用户确认；显式季集唯一匹配才预选，沿用旧秒数默认关闭，新来源进度优先；来源关联原子保存，“我的”可查看关联来源，旧收藏/追更/历史独立保留 |
| 直播 | 纯直播源可只实现 `live`；首页入口显示频道分组、频道分页、频道图标、节目单按需加载；直播解析复用 HLS/普通流、线路/流选择和媒体会话，调用只传 channelId |
| 沉浸式信息流 | 分类页按具体方法显示 feed 入口；首页共用沉浸播放；单条竖屏播放、上下条按钮与方向键、游标分页及 ID 去重；直接使用 entryEpisodeId，缺失时取详情及必要的首批分集；本地收藏、评论与云端互动；按真实分集保存进度，自动下一条遵循偏好；覆盖页面或离开后释放播放 |
| 图文 | `articles.list/getDetail` 独立列表与详情；正文按允许标签重建、图片走所属插件媒体会话、外链使用外部浏览器；支持文内视频、关联点播作品、评论与互动 |
| 我的 | 插件账号资料、云端收藏/点赞/历史分页与本页导入本地；手动批量追更检查每批 ≤50，忽略非本批条目 |
| 云端目录与关注 | 云端收藏目录按原始 folderId 请求，关注列表独立游标分页、头像走所属插件媒体代理；仅实现 interact.follow 时显示取消关注，成功后移除对应作者 |
| 评论与回复 | 详情页按需打开作品/当前真实分集评论，回复使用 replies/commentId；独立分页与 ID 去重、纯文本、头像代理、评论点赞；发送返回评论时插入，无返回则刷新；失败保留草稿，离开页面取消未完成调用；不支持的目标/操作独立隐藏 |
| 云端互动 | 详情页按具体方法显示云端点赞/收藏/作者关注，成功后更新状态；本地收藏单独保留；认证失效清除互动状态并沿用所属插件的登录引导 |
| 上游进度回报 | 实际 playing/pause/ended 产生 start/pause/end；单次播放消费者按单调时钟每 30 秒至多一个 heartbeat；刷新/切流不重复 start，带真实 episodeId 和有限秒数；失败静默，认证错误只清除账号缓存不弹出播放提示 |
| 插件表单 | `auth`/`ui` 必需方法检查、返回状态/字段/资料校验；插件设置与日志面板内展示登录、账号、退出与自定义操作；八种字段、多步值合并、隐藏挑战更新、必填校验、验证码/头像媒体代理 |
| 登录状态 | 插件可保存令牌，宿主按插件和 HTTPS origin 保存网页登录会话。表单输入仅保留在内存中。AUTH_INVALID 保留填写值，AUTH_REQUIRED/EXPIRED 清除页面账号缓存并引导登录。退出失败后重新查询插件资料 |
| 播放恢复 | 页面按到期时间/致命播放错误请求宿主共享刷新；同一会话版本只运行一次解析链，保留源/解析器选择和当前播放位置；成功后原子替换资源，旧地址返回失效；单个等待者取消互不影响，全部等待者取消或会话释放会停止刷新；失败保留旧映射，支持手动重试 |
| 连播准备 | 当前播放稳定后准备下一集及必要分集页；使用已有 reason:preload，预备会话不标记正在播放；手动、自动和手势切集接管同一个准备任务/会话；过期或失败回退正常准备 |
| 有限媒体预取 | 单个低优先级任务；MP4 首部 2 MiB 与尾部 512 KiB；静态 HLS 首个变体、对应音轨、续播点的两个分片及 key/map；缓存随会话隔离，单会话 16 MiB、全局保留 64 MiB、120 秒失效 |
| 播放选择 | 源/解析器各自的候选流和播放线路控件；按阶段与插件 ID 保存原始 serverId；手动切换保留当前秒数，刷新保留选择；解码失败刷新一次后按设置有限切换候选流 |
| 解析器站点偏好 | 播放页主机由宿主提取；点播与共用播放器提供解析器选择及默认关闭的“记住”；成功选择后原子保存，跨站点不复用手动选择；流指定解析器优先，停用/移除/不匹配的记忆不参与自动路由；设置可逐项忘记或清空 |
| 开发资料 | 完整草案类型 `src/plugins/protocol.ts`；可注入传输/加密/HTML 适配器的内存假宿主 `src/plugins/mockHost.ts`；纯虚构源和真实宿主 stdio 测试适配器 |

初始化专项 `tests/runtime_initialization.rs` 覆盖挂起 Promise、持续微任务的 5 秒总截止时间、超过 1 秒但每段未超时的异步工厂、同步死循环及工厂 I/O 禁止。`runtime::initialization_tests` 验证取消挂起/同步循环/微任务循环的初始化后，专用线程及时释放其网络资源。

`tests/module_initialization.rs` 同样覆盖模块顶层 await 的挂起、持续微任务和单段同步死循环；多个短同步片段累计超过 1 秒可正常导入，预览与仅安装均不调用工厂。导入检查不会将无待执行任务的 Promise 提前报为引擎错误。

日志与卸载专项：`native::logs` 检查统一速率、环形容量、包含 details 的序列化字节限制和丢弃计数；`tests/log_limits.rs` 从真实 QuickJS 同时产生 console、校验与业务错误，确认共用同一限额。`runtime::lifecycle_tests` 验证正常清理、禁止卸载 I/O、通知抛错/死循环/挂起 Promise 均可完成停用，已中断运行时跳过通知。日志文件导出仍未接入界面。

当前发现并调度的方法为：

- `media.getDetail`、`media.resolvePlayback`、`media.listEpisodes`、`media.matchUrl`
- `resolver.resolve`
- `search.search`、`search.getFilters`、`search.suggest`、`search.trending`
- `home.getSections`、`home.listSection`、`catalog.getCategories`、`catalog.getFilters`、`catalog.list`
- `library.listMedia`、`library.listFolders`、`library.listFollowing`、`progress.report`、`updates.check`
- `comments.list`、`comments.send`、`comments.like`、`interact.like`、`interact.favorite`、`interact.follow`
- `feed.list`、`endpoints.list`、`transformResource`
- `live.getGroups`、`live.listChannels`、`live.resolve`、`live.getProgrammes`
- `articles.list`、`articles.getDetail`
- `auth.getMethods`、`auth.submit`、`auth.getProfile`、`auth.logout`
- `ui.getActions`、`ui.submit`

当前启用检查要求 source 实现 `media` 或 `live` 的全部必需方法，resolver 实现 `resolver.resolve`。
纯直播源不得声明返回点播 MediaItem 的 home/catalog/search/feed/library 能力而不实现 media。
声明 `auth` 时必须实现全部四个方法，声明 `ui` 时必须实现两个方法；不完整命名空间初始化失败。
上述方法已接入点播详情、播放器、信息流和“我的”的对应用户流程。
未实现的可选特性仍未宣称支持。

表单管理复用 `hpp_manage` IPC：`form.actions` 的 target 为 `auth` 或 `ui`；
`form.submit` 的 target 为 `{ kind: "auth" | "ui", id }`，values 为字符串键值对象，
两者都使用界面预生成的 requestId，进入与普通能力相同的取消/截止时间作用域。
表单内单个非法字段使该表单无效；动作/登录方式列表中的坏条目独立丢弃。
校验日志只含字段路径和原因，不含输入值。以下验证使用离线页面和合成标记。

- Windows 已在真实 Tauri/WebView2 窗口验证三个独立数据目录。三个窗口同时打开同一 origin，分别写入 Cookie 与 localStorage，再逐一读回各自的值。可用 `cargo run --manifest-path src-tauri/Cargo.toml --example webview2_login_isolation -- <新的临时目录>` 复测，进程退出后移除该测试目录。
- Android API 35 / WebView 110.0.5481.154.1 实测 `MULTI_PROFILE=false`、`GET_COOKIE_INFO=true`。`ProcessLoginIsolationTest` 保持主视图打开，先后绑定两个冷启动的同包登录进程，每次在创建 WebView 前设置随机数据目录后缀。两个会话从空状态开始，均能写入各自的标记。主视图在两次登录结束后均保留原值。测试服务仅进入 debug 构建。该测试覆盖串行登录。
- Android API 37 模拟器此前通过默认视图与两个 AndroidX 临时 Profile 的同源隔离测试。

Rust 网络层已实现按插件及 HTTPS origin 保存的宿主管理会话，覆盖普通罐兼容、同源代发、响应头隐藏、登出和在途换号。Rust 运行时仅向用户提交的认证表单开放登录窗口。QuickJS 离线测试通过原生适配器夹具验证了并发互斥、预算暂停与恢复、取消、窗口超时、完成地址校验和返回字段。

Windows 客户端使用独立的 WebView2 InPrivate 窗口，每次登录创建临时数据目录。Android 28 及以上且 WebView 支持 `GET_COOKIE_INFO` 时，客户端使用独立进程，在创建 WebView 前为本次登录设置数据目录后缀。这两端在原生适配器可用时声明 `webview`。无 `form` 的登录方式可由用户点击直接提交。

真实站点登录已在 Windows 和 Android 上由用户手动验证。Windows 登录窗口在独立 STA 线程中运行，顶部显示当前站点和“关闭”按钮。点击该按钮或标题栏的 × 时，宿主在销毁视图前读取当前地址和 Cookie，按 `doneWhen` 判定完成或取消。Android API 35 / WebView 110 模拟器保留数据重装后，插件账号区仍能通过同源请求显示账号昵称和“退出登录”。

播放解析链上的认证错误由宿主标注实际提供者，界面据此进入数据源或解析器的账号区块；
不会信任插件 details 中自报的其他插件身份，也不把解析器登录态与源登录态混用。

## 仍需完成

1. **分发平台完善**：订阅、启动自动检查、自动更新偏好、24 小时冷却及播放期间延后安装已实现并有 Rust 回归。Android content URI 单文件读取已修复实测；相对列表已接入系统目录授权及按文件名逐层解析，不拼接提供者文档 ID。Rust 回归覆盖仅读取所选插件、身份校验、条目失败隔离和取消；Android 路径单测覆盖中文、编码、子目录与越界。目录选择器和不同提供者的设备验收仍待完成，使用范围见[分发指南](../developers/distribution.md)。
2. **其他能力**：云端收藏目前使用上游默认目录；详情互动中的目录选择、分集点赞和更完整账号状态联动后续完善。信息流复用经典播放器的手势与控制器，仍需更多设备验收。
3. **播放完善**：默认清晰度偏好、过期候选索引重新定位、服务器失败切换；完整字幕格式处理。当前候选流有限切换只处理播放器致命错误，解析阶段的认证/锁定错误停止并引导用户处理。
4. **刷新体验验收**：宿主同会话版本共享刷新、原子资源替换与独立取消已实现；设备上的后台恢复与播放连续性仍需验收，未实现的下载消费者不在支持范围内。
5. **媒体预算与缓存**：播放与图片分别限制为 6/2 个传输；当前媒体请求取消推测性预取，排队或传输响应体期间跳过新预取，预取也占媒体传输许可。下一集已有有限缓存。跨会话持久缓存、长时间直播及低延迟 HLS 验证仍未完成；下载队列随未支持的下载能力保留为可选缺口。
6. **字节区间变换**：带字节变换的 HLS BYTERANGE 当前明确返回 UNSUPPORTED；不对截断密文直接解密。
7. **API 与可选功能**：RSA 必需 API 已实现，并通过真实 QuickJS 的两种 PEM/三种填充、随机性、明文/密钥边界与 requires 回归；网页登录已接入 Windows 和受支持的 Android 设备，其余平台未声明 `webview`。DASH 和下载为可选能力，当前未实现。hg.env.media 已接入当前 WebView 播放路径的能力 API 探测；仍需各平台实际媒体与后台恢复验收。
8. **应用功能**：插件自检、数据清理和占用、应用更新。此项目不开发旧平台数据迁移。
9. **平台验收**：Windows 原生窗口和五端完整媒体回归仍未完成；Android 已定位并修复文档选择器 URI 导入失败，粘贴与文件导入新 APK 实测成功。

API 响应缓存当前只接纳显式 `credentials:"omit"` 且无 Authorization/Cookie 的 GET；
对默认 include 但当时恰无 Cookie 的请求采取保守的缓存未命中，不改变响应语义。在途合并不要求启用响应缓存：默认 include 且当前请求不携带 Cookie 时也可合并；重定向到带 Cookie 的目标后，各等待者分别请求。

网络专项 `network::scheduling_tests` 使用真实 HTTP 慢响应覆盖合并、独立取消/超时、认证与有效头隔离、响应体并发上限及跨队列冷却。`tests/media_scheduling.rs` 验证当前播放打断预取、六个媒体响应体占满后排队及传输结束后恢复预取；`tests/playback_performance.rs` 继续验证 MP4/HLS 预取缓存命中。
假宿主不模拟 QuickJS 隔离、完整配额或异步调用上下文；这些必须使用 Rust 宿主测试。

### 播放能力探测（2026-11-16）

- WebView 首次插件命令前完成探测并向宿主注册；所有首批命令共享注册任务，失败时不继续用默认能力调用插件，下一次操作可重试。Tauri 启动不再提前用固定能力初始化插件。
- 普通视频使用 `HTMLMediaElement.canPlayType`，仅接受编码类型的 `probably`。代表性 MP4 编码为 H.264 `avc1.42E01E`、HEVC `hvc1.1.6.L93.B0` / `hev1.1.6.L93.B0`、AV1 `av01.0.04M.08`；同时检查 AAC `mp4a.40.2`。其他容器、音轨与 profile 未据此全面承诺。
- HLS 与 VideoSurface 共用路径选择：优先采用 `Hls.isSupported()` 对应的 MSE 路径，并用 `Hls.getMediaSource().isTypeSupported` 检查编码与 AAC；包含 Hls.js 选择的 ManagedMediaSource。否则检查原生 HLS MIME 支持，并复用原生编码探测。宿主没有 DASH 代理，仍不声明 DASH。
- 某路径至少检测到一种编码才列入 formats。由于协议没有逐格式的编码矩阵，codecs 取已启用路径的交集，按 H.264、HEVC、AV1 排序；交集可以为空。这是保守可用集合，不是所有系统解码器的并集。不会因原生视频能播 HEVC 就宣称 Hls.js 路径也能播。
- 注册结果仅保存在当前宿主进程内。插件工厂取得深层只读快照；点播、直播和解析器的 codecs 由宿主按对应运行时快照填入，覆盖调用者旧值，正常播放/预加载/刷新一致。重复注册相同结果不重建、不打断会话；结果变化时释放旧会话、取消旧运行时并更新修订，下次使用重新初始化。用户数据、插件设置与存储不被清除。
- 无播放前端的 Rust/stdio 宿主与内存假宿主默认报告空能力，测试需显式注入。原生/前端探测与快照传递有逻辑回归；没有额外下载或试播探测视频，不能把能力 API 或编译结果作为五端实际解码证据。流选择偏好与故障恢复仍按现有流程处理。

### 连播与首播优化（2026-11-16）

- 预备会话最多保留 90 秒且不超过来源有效期；临近结尾可更新过期结果。插件修订、线路/解析选择、账号变化、离开与进入后台会取消或失效。预取不弹出认证或选择界面，正式播放时沿用正常错误处理。
- 媒体预取单次最多 15 秒；被当前播放打断后最多尝试 3 次、间隔至少 10 秒，已完成资源复用。缓存不跨会话共享；代理/安全策略变化后不命中旧缓存，释放会话取消传输。
- MP4 来源忽略 Range、需要整资源变换时跳过字节预取。只有强 ETag 或 Last-Modified 及完整 Content-Range 校验通过才拼接缓存前缀与上游后缀，否则正常重新请求。HLS 动态媒体清单不作为静态缓存，BYTERANGE 分片不预取；不承诺所有自适应码率都命中。
- 切集等新会话准备好再释放旧会话；本地进度读取与准备并行，保持视频元素，HLS 设置续播 startPosition、显式 Worker 资源及 30 秒 backBufferLength。预取不创建第二个播放器。
- `media.preload` / `media.preload.cancel` 为宿主内部 IPC，未增加插件 API；协议原有 `reason:preload` 不需扩展。诊断仅保留最多 200 条会话 ID、阶段与耗时，不记录取流 URL、Cookie 或令牌。
- 自动化证据：`scripts/test-next-episode.mjs` 检查任务复用、过期及准备/接管/预取取消；`tests/playback_performance.rs` 用本地真实 HTTP 服务检查连接复用、MP4/HLS 缓存命中、重复预取复用、版本变化/忽略 Range 回退和取消。用户已反馈 Windows/Android 播放和连播顺畅；没有真实站点前后首帧耗时对比，不代表全平台完整验收。
- 加载态按当前会话管理，准备新会话时不显示旧会话的暂停状态；忽略旧播放请求与 AbortError 的迟到失败，实际自动播放权限拒绝仍提供播放按钮。`scripts/test-playback-state.mjs` 覆盖播放请求竞争、显式暂停、暂停时跳转及组件销毁。

分发请求使用独立、匿名的宿主下载环境；远程列表重定向后按最终 URL 解析相对路径，
只下载勾选条目，文件身份与列表不一致时仅该条失败。下载、检查和等待安装锁期间支持取消；
文件替换已开始的单个安装完成后再停止后续条目，不将取消解释为回滚上游操作或存储写入。
离开页面后返回的预览会立即释放。原平台专用调试脚本仅保留在被忽略的本地目录，
公共测试入口只依赖虚构源，不依赖私有插件。

## 播放选择的可执行契约

### 范围与触发

对应 05 §7、06 §2/3、08 §7 与 09 §2/5。此处记录宿主 IPC，插件的 PlaybackParams 不增加解析器字段。

### 签名

`hpp_manage("media.prepare", { id, requestId, args, streamIndex?, resolverId?, resolverStreamIndex?, resolverServers? })`

### 请求、响应与环境

- `args.serverId` 仅传入源的 `media.resolvePlayback`；`resolverServers` 为解析器插件 ID 到原始线路 ID 的字符串对象，只有选中解析器的条目会传给其 `resolver.resolve`。
- 两个 `*streamIndex` 为 0–15 的整数，省略使用 0；选择范围由该阶段本次返回的流决定。
- 返回 `PreparedMedia.choices.source` 及可选 `choices.resolver`：`pluginId`、`servers`、`serverId`、`streamIndex`、`streams`；流描述仅含 index、label、height、bitrate、codec、kind、format、serverId。
- 选择描述不含资源 URL、请求头或变换密钥。会话地址仍由媒体代理提供。无需新增环境变量。
- 已得到源候选但首选流格式不支持、解析器不可用或解析失败时，错误附带宿主生成的 `playbackChoices`，界面保留手动选择其他候选的入口；该字段不接受插件自报。
- localStorage `hpp.playback.servers` 使用 JSON 数组 `["source" | "resolver", pluginId]` 的序列化值作为键，值为原始 serverId；只记录成功解析后实际选中的线路。

### 校验与错误

| 条件 | 结果 |
| --- | --- |
| 索引为负数、字符串或大于 15 | INVALID_ARGUMENT，不执行插件 |
| 索引超出本次候选列表 | 明确报错，保留手动选择控件 |
| 流声明 resolverId，同时存在旧 UI 选择 | 按流声明路由；声明的解析器不可用则明确报错 |
| resolverServers 包含其他解析器条目 | 忽略非当前提供者条目，不转交给源 |
| 源/解析器返回非法线路条目 | 独立丢弃；重复 ID 保留首条；移除未知字段 |
| AUTH_REQUIRED、AUTH_EXPIRED、CONTENT_LOCKED | 停止解析，不自动轮询候选 |

### 正常、基线与异常案例

- 正常：源第 2 条 page 候选 → 指定解析器第 2 条直接流；源和解析器使用不同线路 ID，并分别展示。
- 基线：只有一条源流且没有解析器，隐藏额外控件；不发送解析器 serverId。
- 异常：解码致命错误先刷新一次，开启“播放失败后尝试其他播放流”时每个剩余候选本轮至多尝试一次，耗尽后提示手动操作；用户重试才开启新一轮。

### 回归断言

Rust `tests/playback_selection.rs` 检查两阶段参数归属、指定解析器优先、解析信息合并、选择信息不泄露资源配置、非法索引和坏条目处理。
Edge 集成测试检查真实流/线路切换保留秒数、两阶段选择刷新后保留、跨集与重载持久化、一次刷新与有限候选重试、首选格式不支持后手动播放备用候选。

### 错误与正确示例

错误：把解析器线路放入 `args.serverId`，导致源收到其他插件的线路 ID。

正确：`args.serverId = "source-backup"`，`resolverServers = { "fixture.resolver": "resolver-backup" }`，各阶段只收到自己的原始 ID。

## 直播与图文的调用约定

### 适用范围

对应 05 §9、06 §8/9、08 首页与详情界面、09 媒体归属。首页按具体方法显示来源的直播/图文入口，停用来源不显示。

### 调用签名

- `hpp_manage("live.prepare", {id,requestId,args:{channelId,serverId?,reason,codecs},streamIndex?,resolverId?,resolverServers?,resolverStreamIndex?})`
- `hpp_manage("article.prepare", {id,requestId,args:{articleId,reason,codecs},streamIndex?,resolverId?,resolverServers?,resolverStreamIndex?})`
- 两者返回与点播相同的 `PreparedMedia`，释放仍使用 `media.release`。

### 参数与资源契约

直播只向 `live.resolve` 传原始 channelId、播放参数，不构造 mediaId/episodeId。节目单使用 Unix 毫秒窗口，用户打开后才请求未来 24 小时。
图文播放重新请求 `articles.getDetail({id:articleId})`，仅取其中 streams 进入共享媒体链；正文/作者等字段不进入播放配置。
图文评论与互动分别使用 `{kind:"article",articleId}`，不复用 media 目标。两类播放沿用来源/解析器隔离、取消、修订和字幕归属，无新增环境变量。

### 校验与失败处理

| 条件 | 结果 |
| --- | --- |
| source 只有完整 live | 正常启用，不要求空壳 media |
| live/articles 命名空间缺少必需方法 | 初始化失败，保留可查看的安装记录 |
| 频道非法 ID/number，节目时间缺失或倒置 | 丢弃独立坏条目，记录路径 |
| 图文正文不是文本 | 结果校验失败 |
| 正文超过 512 KiB | 按 UTF-8 字符边界截断并警告 |
| streams 为 [] | 正文正常显示，不显示视频入口 |
| 节目单失败 | 不影响频道播放，提供独立重试 |
| 播放致命错误 | 尝试一次 reason:refresh，失败后允许手动切换/重试 |

### 正常与异常用例

- 正常：纯直播源 → 频道分页 → 点播频道 → 原生代理 HLS → 原始 signal ID 切换 → 手动刷新。
- 基线：只有纯文本的文章，按纯文本渲染，无需视频或评论能力。
- 异常：文章含 script/style/iframe/form、事件属性或 javascript 链接，整个危险节点或属性不参与可见节点构造；正文图片必须先登记代理 URL，不能直接填入远程 src。

### 必须验证的行为

`tests/extra_content.rs` 覆盖纯直播初始化、缺失方法、非法独立条目、正文预算、空 streams、媒体会话与公开选择描述。
`scripts/test-plugin-extra-content.mjs` 覆盖生产 App 的首页入口、频道/文章分页、HLS/普通视频、节目单按需请求、原始参数归属、正文清洗及外链命令。
列表封面集中登记图片批次，避免每行占用一个媒体会话；超出 200 个图片的集合分批登记并在页面销毁时释放全部会话。

### 实现示例对照

错误：直播插件伪造 VOD episodeId；文章 HTML 原样放入界面并保留上游 img src。

正确：频道仅传 channelId；文章使用惰性模板解析，再从允许的节点/属性构建 React 元素，图片统一替换为 assets.prepare 返回的本地地址。

## 跨来源确认与片库关联

对应 06 §17.2、08 §6/7，不改变插件 search/media 的公开签名。用户打开换源面板后按当前标题并行查询其他已启用、可见且实现 search/media 的来源；年份与类型用于宿主排序，不增加插件未约定的搜索参数。

### 关联命令

- `hpp_manage("library.sources.get", {pluginId,mediaId}) -> LibraryEntry[]`：返回关联组内其他条目，包含来源不可用的历史条目。界面按当前可用来源与成人内容偏好决定能否显示和打开。
- `hpp_manage("library.sources.link", {from:{pluginId,item:MediaItem},to:{pluginId,item:MediaItem}}) -> null`：仅在用户确认后调用，校验两侧再一次原子写入；已有两个关联组会合并，重复关联幂等。
- `library.json.associations` 为条目键到关联组条目键集合的映射；键仍使用 JSON 序列化的 `[pluginId,mediaId]`，不拼接分隔符。原有 favorite、tracking、lastProgress 和逐集 progress 均不转移、不清除。
- 无新增环境变量；不需要、也不执行旧 HGhub 数据迁移。

### 确认后播放

候选详情所有已返回播放列表中，明确的 `season + episodeNumber` 仅匹配一集且无未加载分页时可预选；双方都缺少 season 时允许按明确 episodeNumber 预选。index 相同、身份缺失、多个匹配或分集尚未全部返回时不自动选集。

用户可手动选新来源分集、加载其后续页，或只打开新来源详情。锁定分集不可选。
跨页选择只保存真实 playlistId/episodeId；新页重新取详情并沿原始游标查找该分集，重复游标终止，分集变化时停止并要求重选。

目标分集已有本地进度时优先使用目标记录。没有时，“沿用原来源播放位置”默认不勾选；用户勾选才复制确认时有限、非负的秒数，否则从 0 开始。
确认前先保存原来源当前进度，再建立关联并替换当前详情路由；取消、搜索、查看候选均不建立关联，也不触发新来源解析。

### 校验与失败边界

| 情况 | 行为 |
| --- | --- |
| 关联任一条目标识或 MediaItem 非法 | INVALID_ARGUMENT，两侧记录与磁盘均不变 |
| 关联到自身 | INVALID_ARGUMENT |
| 一个来源搜索失败 | 本来源独立错误与重试，其他候选可继续选择 |
| 候选详情或分集页失败 | 留在当前来源，显示重试，不解析候选媒体 |
| 新来源进度读取失败 | 停止确认播放，避免在未知优先记录时覆盖进度 |
| 关联保存失败 | 保留原详情与播放，显示失败 |
| 新来源分集已移除、锁定或重复游标后仍找不到 | 不自动播放，提示用户重新选集 |
| 插件移除 | 关联与旧片库保留，界面标记来源不可用 |

正常案例：A 的 S2E1 → B 的明确 S2E1（即使 index 为 41），用户确认后以 B 的真实 ID 播放。基线案例：不勾选沿用位置且 B 无进度，则从 0 播放。异常案例：B 中有两条 S2E1，要求手动选集；同 index 不视为匹配。

Rust `content_library` 检查关联组对称/合并/幂等、重启恢复、非法写入原子性，以及各来源收藏/追更/进度独立。`scripts/test-plugin-source-switch.mjs` 使用真实 QuickJS 与生产界面检查排序、失败隔离、取消、季集匹配、进度优先级、显式沿用秒数、跨页选集及“我的”关联入口。

错误示例：根据旧 `index:1` 把新来源第一个条目视为同集，并直接覆盖旧记录。
正确示例：只用明确季集身份提出候选，由用户确认新来源及分集，各来源继续按自己的真实 ID 记录。

## 信息流的调用约定

对应 05 §3、06 §5、08 §7 与 09 媒体会话约定，不新增插件能力或字段。

- `feed.list({cursor?,limit:20})` 的条目按 ID 去重；下一页失败保留当前播放条目，重试使用原始游标。重复游标停止继续加载；仅有重复条目的页不将索引推进到空条目。
- 存在 `entryEpisodeId` 时直接调用 `media.prepare`，`args` 为真实 `mediaId`、`episodeId` 和通用播放参数，省略未知的 `playlistId`；不会为播放而额外调用 `media.getDetail`。
- 缺少直接入口时请求 `media.getDetail({id})`，取第一播放列表的首集。列表首批为空且有 `next` 时按原始 `playlistId` 与游标调用一次 `media.listEpisodes`；锁定或仍无分集时显示错误与详情入口，不用 `index` 伪造分集 ID。
- 无播放列表身份的直接入口进度使用 `[pluginId,mediaId,null,episodeId]` 为键，与包含真实 `playlistId` 的进度分别保存；不要求迁移旧平台数据。
- 播放开始后按周期、暂停、结束及离开保存本地进度。上游报告沿用实际播放事件和 30 秒心跳约定，刷新不重复 start；报告失败静默。
- 每次只挂载一个播放消费者；切换、覆盖或离开释放媒体会话，返回时从已保存位置继续。自动下一条受应用偏好控制；末页不循环。
- 回归使用 `tests/fixtures/feed.js` 和 `scripts/test-plugin-feed.mjs`，通过真实 QuickJS、媒体代理和生产 React 页面验证。浏览器入口启用与生产一致的 React StrictMode，分类元数据不缓存已取消的请求 Promise。

## 首页编排与滚动分页

对应 08 §3–5 与 03 §3，不新增插件方法。

- `hpp.preferences` 记住来源顺序 `order`、首页隐藏来源 `hiddenHomeIds`、分区顺序 `sectionOrder` 和分区显隐 `hiddenSections`；首页显隐不改变搜索范围。多个来源模式仍保留选择顺序，拖动来源时同步该模式的已选顺序。
- 推荐列表的“调整”提供原生 HTML 拖动把手与可访问的上移/下移按钮；当前首页直接播放，推荐列表由分类页入口进入。分类沿用来源顺序；“全部参与”只恢复该来源分区，不修改其他来源。
- 推荐分区、分类和分区列表共用 `usePagedPlugin`，搜索保留各来源独立的请求锁和游标链。按原始 ID 去重，已消费游标不能再次请求；来源返回重复游标时停止并显示提示。
- 底部哨兵以当前滚动容器为根预加载，同时保留按钮。下一页失败不会清空已有内容，也不会自动无限重试；重试使用失败时的原始游标。首页刷新失败保留现有推荐。
- 查询、筛选、排序、插件修订或搜索范围变化时取消旧请求，旧响应不能覆盖新结果。分类树仅缓存成功的结果，键含插件修订；不缓存或跨页面共享未完成的请求 Promise。
- `scripts/test-plugin-browsing.mjs` 使用真实拖动、滚动与生产页面验证，夹具在 `tests/fixtures/browsing.js`（HPP crate 内）；全程使用不透明游标，不将其转换为页码。

## 按播放站点记住解析器

对应 06 §3、08 §7/8，不更改插件的 `PlaybackParams` 或解析器参数。

### 宿主命令与公开选择描述

- `resolver.preferences.get({}) -> Record<string,string>`：返回精确主机名到解析器插件 ID 的映射。
- `resolver.preferences.set({host,pluginId:string|null}) -> null`：只接受当前可用的解析器；null 忘记该主机。拒绝 URL、端口、路径、通配符与非法类型；主机规范化为小写/IDNA，去除 DNS 末尾点。
- `resolver.preferences.clear({}) -> null`：清空全部记忆。`resolver-preferences.json` 独立于插件数据，先原子写入成功再替换内存；写入失败保留原值。没有新增环境变量。
- `PreparedMedia.choices.resolverRoute` 与宿主生成的 `playbackChoices.resolverRoute` 包含 `host`、`forced`、`selectedId?`、`resolvers:[{id,name}]`。界面只取得主机和可用选项，不取得播放页路径、查询参数、请求头或密钥。
- `media/live/article.prepare` 的可选 `resolverHost` 将当前手动 `resolverId` 限于对应主机。源在刷新或换集后改用其他播放站点时，旧的界面选择不参与新站点路由；这两个字段不会传给源或解析器插件。

### 优先级与持久化

1. 流声明的 `resolverId` 优先，指定插件不可用时明确失败，不自动改用其他解析器。
2. 本次对当前主机的手动选择优先于记忆。
3. 未手动指定时，从可用且 `matches` 命中的候选里取该精确主机的记忆；停用、移除或不匹配的记忆忽略。唯一匹配直接使用，多个匹配且无可用记忆则要求选择。
4. 没有匹配时列出已安装可用解析器供手动选择；不将匹配其他主机的历史偏好自动套用。
5. “记住这个站点的选择”默认关闭，仅本次解析成功、实际提供者与所选项一致且主机未变化时保存；失败或取消不覆盖旧记忆。插件指定解析器时不提供覆盖记忆入口。

点播详情与直播/图文/信息流共用该选择流程；设置提供逐项忘记与全部清空。
`tests/resolver_preferences.rs` 验证精确主机隔离、跨宿主重启、指定优先、失效偏好和落盘失败；
`scripts/test-plugin-resolver-preferences.mjs` 验证实际点播/直播、默认不记住、跨站点切集、失败不覆盖、页面重载与设置清理。

## 验证与复现

在仓库根目录执行：

```powershell
cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
cargo build --manifest-path src-tauri/Cargo.toml
npm.cmd run build
npm.cmd run test:plugins
```

依赖已缓存时可以为 Cargo 添加 `--offline`。浏览器测试默认使用 Windows Edge；
其他安装位置可设置 `BROWSER_EXECUTABLE`。测试数据目录临时创建，不读取正式插件、登录态或上游。
exFAT 工作目录不支持硬链接；如 MSVC 报增量缓存调试信息损坏，可设置 `$env:CARGO_INCREMENTAL = '0'`
后重新构建。资源紧张时为 Cargo 添加 `-j 2`。

`test:plugins` 运行真实 Rust/QuickJS 宿主和生产 App 与 React 页面，只将 Tauri IPC 换成 stdio 传输。
浏览器本地录制合成 H.264 画面，再经实际媒体代理验证普通视频拖动、HLS 初始化段/分片播放及刷新。
新增回归覆盖新导航、首页、分类筛选/排序、多来源搜索合并与单源失败隔离、本地收藏/追更、实际分集续播。
同时使用 `src-tauri/crates/hpp/tests/fixtures/forms.js` 验证账号/操作表单、多步登录、登录态恢复与失效、
退出接口失败后的本地会话清理，以及离开面板取消提交。注册入口只验证通用外链命令，不访问外部站点。
`src-tauri/crates/hpp/tests/fixtures/social.js` 与 `scripts/test-plugin-social.mjs` 验证评论/回复、云端互动、目录与关注分页、取消评论，以及实际播放事件回报；心跳使用可注入的单调时钟验证 30 秒边界。
直播与图文回归使用 `src-tauri/crates/hpp/tests/fixtures/extra-content.js`；纯直播源不声明点播 media，以防测试空壳掩盖初始化约束。
本地私有开发基线独立验证，不纳入客户端、公共测试或文档站产物。
产物位于忽略提交的 `test-results/plugins/<时间>/`，包含测试报告、截图和本地请求记录。
这项测试不等同于实际 Tauri WebView2 窗口/打包验证。

## 历史验证（2026-11-13，早期界面）

- HPP Rust 测试 46 项全部通过，含无 playlist 的信息流进度隔离与持久化、关联来源合并/幂等/原子性、解析器站点偏好的持久化/优先级/失效回退/写入失败保留；评论/互动、直播/图文结果过滤、纯直播能力发现、媒体会话、安装、账号、播放及网络回归通过。
- Tauri 应用 Rust 日志测试 3 项通过。
- 前端类型检查、生产构建与 Windows debug 可执行文件构建通过。
- Edge 实际播放与生产 App 严格模式回归通过，报告：`test-results/plugins/2026-11-13T05-32-01-958Z/report.json`，包含 25 组通过结果。
- 覆盖首页/分类、多来源合并与来源选择、单源失败隔离、收藏/追更/分集续播、账号表单、
  云端列表导入、追更结果归属过滤、移除插件保留不可用来源记录、分区完整列表导航、线路刷新与探测。
- 分发覆盖文件选择传输、本地相对列表、重定向后的无扩展名远程列表、简介及来源名称、只下载勾选项、
  身份不符条目隔离、仅安装、更新检查不替换文件、失败更新恢复和勾选安装更高版本。
- 播放覆盖源/解析器流和线路切换、保留秒数、刷新与跨集保留选择、重载持久化、一次刷新后的有限候选流切换，
  以及首选格式不支持后手动播放备用流；新增设置可关闭自动候选流切换。
- 评论覆盖作品/分集/回复目标、独立分页、纯文本显示、未登录保留草稿、返回评论插入与无返回刷新、离开页面取消提交，UNSUPPORTED 分别隐藏目标、发送或点赞入口。
- 云端互动覆盖点赞/收藏/作者关注与取消关注，收藏目录原始 ID、关注列表分页；进度覆盖实际播放 start/pause/end、认证失败静默、30 秒心跳边界与重复事件抑制。
- 直播/图文覆盖纯直播源、频道分组/分页、按需节目单、实时线路切换、图文分页、正文 allow-list、图片媒体代理、外链打开、文内视频与文章评论互动。
- 信息流覆盖直接入口不取详情、真实播放列表/分集分页回退、失败重试保留条目、去重和重复游标终止、收藏与暂停/离开续播、刷新不重复 start、自动下一条开关、覆盖页面释放播放，以及导航始终位于窗口内；截图为同目录 `feed-player.png`。
- 修复分类页在 React StrictMode 重挂载时复用已取消请求而无法加载的问题；生产入口与浏览器测试使用相同严格模式。
- 换源覆盖精确/相似候选排序、单源搜索失败隔离、取消不建立关联、明确季集匹配（不同 index）、目标进度优先、默认不沿用旧秒数、显式沿用、含糊集号手动选择、真实跨页分集重定位，以及“我的”关联入口和卸载后保留关系；截图 `source-switch-confirmation.png`。
- 浏览覆盖真实来源/分区拖动、按钮移动、显隐/全部参与、重载持久化与分类来源顺序；滚动触发分页、下一页失败保留内容、原游标重试、ID 去重、重复游标终止、筛选/排序/查询取消、多源独立失败和首页刷新失败保留内容。分类树跨导航复用成功缓存，同版本替换修订后重新获取；截图 `home-arrangement.png`、`browsing-pagination.png`。
- 解析器偏好覆盖默认不记住、勾选且成功才保存、失败不覆盖旧偏好、跨播放主机切集隔离、指定解析器优先、页面重载自动路由、点播/直播共用入口，以及设置逐项忘记/清空和清空后的重载；截图 `resolver-site-preference.png`、`resolver-preference-settings.png`。
- 测试只访问本机虚构服务，没有外部内容站点请求。浏览器 stdio 桥回归不等同于实际 WebView2 窗口验收。

## 2026-11-15 审计与经典界面回归

当前桌面首页与详情入口共用经典沉浸播放器，选集、评论、详情通过抽屉打开；手机沿用文字底栏与播放器手势。信息流 entryEpisodeId 直接解析，打开详情/选集抽屉时才请求详情，不人为添加 playlistId。错误提示不遮挡收藏和选集。

本轮 Rust 全套（含 runtime_contract 与 public_example）通过；经典 UI 的社交、直播/图文、信息流、换源和解析器专项回归通过。Android content URI 文件导入修复后已实测；最终 APK 粘贴/文件导入及单部真实 MP4（1920×804）播放通过。完整前端 25 组、Rust 52 项通过。剩余缺口见本文限制说明，不把历史界面测试当作当前验收。

## 引擎补丁

`rquickjs-sys 0.12.2` 的普通 Promise reaction/await 未提供完整 Before/After 钩子。
本地 vendor 补丁为每条 continuation 捕获注册上下文，避免并发调用或共享 Promise 时串用 requestId。
详见 `src-tauri/vendor/rquickjs-sys/HGHUB-PATCH.md`；升级引擎时必须保留相应回归测试。
