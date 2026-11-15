# HGhub iOS 定制版开发状态

更新：2026-11-15（Asia/Shanghai）
状态：本地核心回归通过，第二轮 macOS 已真实生成并校验未签名 IPA；正在构建最终 iOS 16.4 版本。

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
- IPA 最终结构、Mach-O、Info.plist、图标资源/权限：等待最终版本重建与下载后二次校验。
- iPhone 真机、原生横屏、PiP、长时间内存：未验证，没有连接设备。

## 下一步
继续修复实际 Xcode 编译直到产物可校验；对最终源码重新运行工作流并下载实际 IPA。个人签名/安装、构建与真机清单见 docs/IOS_BUILD.md。

## 继续开发约定
每次继续前先读本文件、根目录 PROJECT_STATUS.md、当前源码及 git status/log。不得将浏览器模拟、构建成功等同于 iPhone 真机通过。
