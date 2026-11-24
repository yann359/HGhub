# 调试与验收

在插件“设置与日志”读取日志，先确认初始化，再调用具体方法。记录 requestId、错误码、字段路径，避免凭据。当前没有完整内置自检、日志文件导出；日志可选中复制。

| 症状 | 检查 |
| --- | --- |
| 安装后运行失败 | 工厂 I/O、必需方法、requires |
| 搜索为空 | 字段映射、条目校验、真实 ID |
| 第二页失败 | 游标、闭包状态、取消 |
| 图片或视频 403 | 资源请求头、Cookie 提供者、CDN 地址 |
| 刷新无效 | reason:refresh、过期缓存、毫秒有效期 |
| 页面流不能播 | 匹配解析器、页面是否误标为 MP4 |
| Android 列表失败 | 相对条目先用“选择列表所在目录”授权，检查文件名、子目录及是否越界；单文件授权不包含兄弟文件 |

## 播放能力测试

`hg.env.media` 是实际播放前端注册的加载快照。单独运行 Rust/stdio 宿主时默认 codecs/formats 均为空；使用 stdio 适配器的测试可先调用宿主动作 `host.media.configure`，参数例如 `{ "codecs": ["h264"], "formats": ["hls", "progressive"] }`。这是宿主测试接口，不是插件可调用的 hg API。内存假宿主通过 `createMockHost({ env: { media: ... } })` 显式注入测试能力，不能据此声称设备支持。

`node scripts/test-media-capabilities.mjs` 检查路径选择、API 返回值、编码交集、启动注册等待及失败重试；Rust `tests/media_capabilities.rs` 检查真实 QuickJS 快照、点播/直播/解析器参数、重复注册与能力变化。它们不进行视频样本解码或 UI 验收。

## 仓库验证

目录导入专项：`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --test distribution --test automatic_updates` 覆盖不透明 URI 适配、只读取勾选条目、身份不符、错误隔离、预览取消及更新回归。运行 `node scripts/sync-android-network.mjs` 后，在 `src-tauri/gen/android` 执行 `./gradlew.bat :app:testX86_64DebugUnitTest -x :app:rustBuildX86_64Debug` 验证 Android 中文/编码/子目录路径及越界拒绝。设备另验系统授权、取消选择、不同文件提供者、重启后重新授权和保留数据安装。

日志/卸载专项：`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --lib native::logs` 检查 240 行/分钟、2000 行保留、单行含 details 的 2 KiB 限制与丢弃计数；`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --test log_limits` 检查插件日志、校验告警和业务错误共用限额。`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --lib runtime::lifecycle_tests` 检查正常卸载通知、禁止 I/O、抛错/死循环/挂起 Promise 的停用耗时，以及中断运行时跳过通知。

初始化专项：`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --test runtime_initialization` 检查挂起 Promise 和持续微任务均受 5 秒总预算限制、多个短同步片段可累计超过 1 秒、同步死循环被中断，以及工厂不可调用 HTTP/存储/Cookie/HTML/定时器。`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --lib runtime::initialization_tests` 检查取消初始化后线程释放资源。

模块顶层专项：`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --test module_initialization` 检查顶层 await 挂起、持续微任务、同步死循环和多个短片段的时间边界，并检查预览及仅安装不运行工厂。模块求值与元信息检查共用 5 秒；启用时工厂另有 5 秒预算。

网络调度专项：`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --lib network::scheduling_tests` 使用真实 HTTP 慢响应检查匿名 GET 合并、实际 query/有效头、独立取消和超时、认证请求隔离、每主机 4/每插件 6 的响应体并发限制，以及媒体独立队列与共享冷却。`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --test media_scheduling --test playback_performance` 检查前台播放打断预取、排队/传输期间跳过新预取、结束后恢复预取，以及 MP4/HLS 缓存复用。

共享刷新专项：`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --test transport shared_refresh` 检查同版本并发只解析一次、单个与全部等待者取消、释放会话、失败保留旧映射及重试、旧 HLS 子资源失效和新版本寻址。`node scripts/test-playback-state.mjs` 检查同一会话换版本会重载视频并恢复当前秒数，旧播放请求不能覆盖新状态；`node scripts/test-next-episode.mjs` 检查连播准备的复用与取消。均不替代设备上的播放连续性验收。

外部流专项包含在 `tests/playback_selection.rs`：六种允许协议、正常/预加载/刷新均只返回确认信息、不创建活跃媒体会话、不泄露头和 Cookie；拒绝脚本/文件/data 地址及 URL 内账号密码，插件不能伪造宿主确认目标。设备另验确认、取消、处理程序缺失、外部打开后返回应用；编译或宿主测试不等于系统启动验收。

字幕专项：`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --lib media::subtitles` 检查 SRT 时间轴、UTF-8、UTF-16 BOM、GBK charset 与非法输入；`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --test transport srt_tracks` 检查真实 QuickJS 到媒体代理的格式声明、轨道元信息、转换后的 Range/HEAD、坏轨不影响视频及会话释放。设备上另需验证默认字幕、多轨切换、关闭字幕与视频继续播放；专项测试不代表设备验收。

```powershell
pnpm install --frozen-lockfile
pnpm build
$env:CARGO_INCREMENTAL = '0'
cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --locked --lib --tests -j 2
cargo build --manifest-path src-tauri/crates/hpp/Cargo.toml --locked --example dev_host -j 2
node scripts/test-plugins.mjs
pnpm docs:build
pnpm docs:test
```

浏览器套件使用真实 Rust/QuickJS 和生产 React，仅用 stdio 代替 Tauri IPC；媒体由本地合成。它不等于原生窗口或实际站点验收。测试期间不要重建 Windows dev_host.exe。

播放体验可单独运行 `pnpm test:experience`（或在已有 dev_host 构建上运行 `node scripts/test-plugins.mjs --experience`）。覆盖首页轻量预览、详情确认、选集分页、预览会话接续、桌面全窗口播放、移动端控制和退出恢复；分类、搜索、收藏及历史入口应先显示详情，不自动播放。截图输出到 `output/playwright/watch-experience/`。原有搜索、设置和移动手势回归运行 `pnpm test:frontend`。

发布前检查：单文件与列表选装、重复导入、版本一致；搜索/首页→详情→真实分集→实际解码；第二页、切线、拖动、刷新、取消；重载与持久化。每个声称支持的平台都应验证本地会话到播放器的完整链路。

假宿主用于字段映射，不能证明隔离、配额、Cookie、媒体代理或原生解码。报告分别记录模拟、真实宿主、上游和设备验证。

## 网页登录

`createMockHost({ webview })` 可注入返回 `{ finalUrl }` 的登录夹具。假宿主会根据该注入设置 `webview` 特性，适合测试登录方式的显隐和站点账号响应。

`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --test web_login` 用真实 QuickJS、认证表单 IPC 和模拟登录窗口验证用户动作、超时、取消及预算恢复。`cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --lib network::web_login_tests` 检查会话隔离、同源请求、登出和换号。

在支持网页登录的客户端，可按以下步骤验收真实站点。

1. 在插件页打开“设置与日志”，在“插件账号”选择网页登录，确认窗口顶部显示预期站点。
2. 在站点完成登录。窗口可在满足 `doneWhen` 后自动返回。若 Windows 窗口仍开着，点顶部的“关闭”或标题栏的 ×，宿主会在关窗前核对完成条件。
3. 回到“插件账号”核对昵称，收起并重新打开“设置与日志”，确认 `auth.getProfile` 仍能从同源接口取回同一账号。
4. 点“退出登录”，确认账号区重新显示登录方式。再发起一次登录，这次在填写账号前点“关闭”，确认此次操作以 `CANCELLED` 结束。

Windows 与 Android 的实际验收环境见[实现范围](../plugin-protocol/IMPLEMENTATION.md)。

## 代理与 Android release

设置 → 网络 → 代理访问支持跟随系统、自定义与直连。自定义支持 HTTP、HTTPS、SOCKS；保存后新请求生效，已缓冲的视频不会重新下载，必要时刷新播放。测试连接只使用当前草稿，不保存。直连关闭应用层代理，但不会绕过操作系统 VPN / TUN。

系统模式使用 `HGHUB_PROXY` 覆盖值或平台固定代理、代理环境变量；Android 读取系统 HTTP 代理属性，Linux 另支持 GNOME 手动代理。PAC 自动配置脚本不在当前支持范围。代理选择仍保留目标地址与重定向检查。

Android 的封面和媒体通过应用内 HTTP 回环服务提供。`scripts/sync-android-network.mjs` 在构建前将受版本管理的网络配置同步到生成工程，只允许 `127.0.0.1` 和 `localhost` 明文连接。重新执行 Android init 后也需同步；验收 release 时检查 APK 内的 manifest 与 XML，不能仅凭 debug 可播放判断 release 正常。

同一同步脚本也会复制 Android 的 `OrientationPlugin.kt`。播放器通过已注册的 `set_orientation` 命令切换 Activity 的系统方向，退出时恢复进入前的方向设置；横屏不再依赖 CSS 旋转。浏览器只能模拟视口尺寸变化，系统方向与状态栏行为需在 Android 上确认。

应用主 Cargo 清单的开发配置禁用增量编译，以避免当前 E: 文件系统不支持硬链接时出现旧增量对象的未解析符号。若已有损坏产物，可先执行 `cargo clean --manifest-path src-tauri/Cargo.toml -p hghub-hpp`，再运行 `pnpm app:dev`。
