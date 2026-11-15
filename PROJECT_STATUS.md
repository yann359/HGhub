# HGhub iOS 定制版开发状态

更新：2026-11-15（Asia/Shanghai）
状态：源码、自动化回归、真实 iOS Release 编译与最终 IPA 双重校验已完成；待个人签名后的 iPhone 真机验收。

## 目标
保留插件视频播放器全部能力，提供 Apple 风格 iPhone UI，构建真实 ARM64 Release 未签名 IPA，仅供非商业学习与个人使用。

## 源码基线与架构
- 用户仓库：https://github.com/yann359/HGhub
- 基线：2379957f7c9ae50f6d60a6da12e6124e91962634（当前插件版 main；版本 0.0.1，并非上游稳定发布）。
- 本地此前 v1.2.1（5f88442）是旧版站点客户端，缺少用户要求的插件架构。上游 Minireel/HGhub 已撤下源码及 Release，不能再核实新的稳定标签。
- 开发分支：codex/ios-liquid-glass-plugin；不修改 main。
- React 19 + TypeScript + Tauri 2；Rust HPP + QuickJS 隔离插件；Axum 本机媒体代理保留请求头、Range、变换和会话取消。
- 复用当前插件导入/启停/更新、来源切换、搜索/分类、收藏/历史/续播及本地片库；不内置第三方插件或内容。
- 许可证：原 PolyForm Noncommercial 1.0.0 及版权声明保持不变。
- 未找到 HGhub-iOS-LiquidGlass-Patch.zip。

## 本次修改
- iOS 专属玻璃视觉、四入口悬浮导航、卡片、底部 Sheet、安全区、44pt 点击目标、动态字体与减少动态效果适配。
- 浅色/深色/跟随系统三种模式，启动脚本避免主题闪烁。
- iOS 原生 HLS，其他平台保留 hls.js；限制 MSE 缓冲；30 秒加载提示；有限恢复；前后台与手动暂停状态；切源销毁、清理监听器和定时器。
- 全屏使用实际 HTMLVideoElement WebKit/Fullscreen API，画中画按运行时支持调用；移除无宿主实现的 set_orientation 调用，不使用 UIDevice KVC。
- QuickJS iOS target 启用 bindgen，并按真实 Xcode SDK 生成绑定。
- 移除虚假 Team ID；不申请任意网络权限；仅声明本机媒体所需 NSAllowsLocalNetworking。
- 最低 iOS 16.4：项目现有 Tailwind CSS 4 的官方 Safari 支持下限为 16.4，且插件选择器需要 dialog.showModal；不声明低于实际依赖支持范围的系统。
- 改造 macOS GitHub Actions、ARM64 release 编译、结构化 IPA 检查、日志及 Artifacts；不自动覆盖正式 Release。

## 验证进度
- 前端类型检查/生产构建：通过；Vite 仍提示主 JS 包约 921KB（gzip 285KB），不是构建失败。
- Windows Rust 主程序 `cargo check --locked`：通过。
- Windows Rust 主程序 3 项单元测试：通过。
- Rust HPP：本机 52 项测试通过；macOS 首轮全部通过。
- 完整插件/播放器集成回归通过：真实 Rust/QuickJS + React + Edge，仅替换 Tauri IPC，使用虚构插件/原创合成 MP4/HLS；包括导入、更新失败恢复、启停、鉴权、多源、线路、选集、直播、收藏/历史/续播、Range/取消和会话释放。
- Windows/Android 界面回归与 iPhone 尺寸回归通过；iOS 44px 导航、320px/横屏无溢出、三种主题与模拟前后台验证完成。注意：UA/尺寸模拟仍是 Chromium，不能代替 WKWebView 真机。
- macOS 首轮 43717039167 参数服务器等待超时已修复；第二轮 43717838058 成功，真实 ARM64 Release 未签名 IPA 已通过首版结构检查并上传 Artifacts。
- 中间产物：https://github.com/yann359/HGhub/actions/runs/43717838058/artifacts/14163682045（源码 9282462，最低 15.4；仅作构建证据，最终交付将以 16.4 版本替代）。
- 完整云端日志：https://github.com/yann359/HGhub/actions/runs/43717838058/artifacts/14163552451。
- iOS 清屏隐藏控制栏时保持视频边界不变的回归通过，避免画面跳动。
- 最终 macOS 构建 43718975540：全部成功；Xcode 16.4 / iOS SDK 18.5 / Rust 1.100.0 / Node 22.23.4 / pnpm 10.34.9。
- 最终 IPA 已下载并在 Windows 再次通过结构校验；13 个 ZIP 条目，5,289,728 字节，真机 ARM64、Release、PIE、未加密、无签名与描述文件；Info.plist 与 Mach-O 均声明最低 iOS 16.4，图标/Assets.car 存在，ATS 无任意访问例外。
- IPA 对应应用源码提交：88b472a65a671b17cd15d03f5a82157f8dca753f。其后提交仅补充交付文档，不改变应用代码。
- SHA-256：150416507140c9f0c26157c9ed4c0ed1743c576fb718c9f5a67e18af206c0b19。
- iPhone 真机、原生横屏、PiP、长时间内存：未验证，没有连接设备。

## 最终产物与下载
- 构建与完整在线日志：https://github.com/yann359/HGhub/actions/runs/43718975540
- IPA / manifest / SHA256SUMS：https://github.com/yann359/HGhub/actions/runs/43718975540/artifacts/14163871595
- 完整诊断日志与 xcresult：https://github.com/yann359/HGhub/actions/runs/43718975540/artifacts/14163004758
- 预览 Release（含 IPA、校验记录、完整日志和本地回归证据）：https://github.com/yann359/HGhub/releases/tag/v1.0.0-ios-liquidglass-preview
- IPA 直接下载：https://github.com/yann359/HGhub/releases/download/v1.0.0-ios-liquidglass-preview/HGhub-iOS-LiquidGlass-v1.0.0.ipa
- Actions 产物保留 30 天（截至 2026-12-15）；Release 附件提供持续下载，仍需个人签名。
- 本地交付目录：工作区 dist/ios；包含最终 IPA、云端及本地校验结果、日志 ZIP、浏览器回归报告/截图和源码归档。
- 本地业务回归 25 组、界面回归 4 组，全部通过；测试不请求外部内容源。

## 未解决问题与风险
- 未连接真机，不能声明已实际安装成功；原生 HLS、系统横屏/退出全屏、PiP、文件选择器、安全区、动态字体、音频中断与连续 60 分钟内存需要真机验收。
- 画中画由设备、系统和当前视频运行时能力决定；不支持时给出提示，不提供假按钮效果。没有启用后台音频权限。
- 插件版基线为早期 0.0.1，第三方插件的接口实现、合法授权、网络条件及编解码能力影响可用性；未附带第三方源。
- Vite 主包仍约 921KB，后续可单独评估拆包；本次优先维持播放器与插件稳定性。
- Windows 编译/浏览器回归及 Android 视口回归已通过；没有执行 Android APK 构建或安卓真机测试。

## 下一步
使用个人 Apple 身份签名后，在 iPhone 按 docs/IOS_BUILD.md 清单验收；记录设备/系统/插件/编码格式，收集脱敏日志。完成真机验收后再考虑正式版标记与性能优化。

## 继续开发约定
每次继续前先读本文件、根目录 PROJECT_STATUS.md、当前源码及 git status/log。不得将浏览器模拟、构建成功等同于 iPhone 真机通过。
