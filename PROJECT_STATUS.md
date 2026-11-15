# HGhub iOS 定制版开发状态

更新：2026-11-15（Asia/Shanghai）
状态：开发与实际构建验证中，尚未生成或验证 IPA。

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
- 最低 iOS 15.4：现有插件选择器使用 dialog.showModal，该能力需要 iOS 15.4；不静默假装支持 15.0。
- 改造 macOS GitHub Actions、ARM64 release 编译、结构化 IPA 检查、日志及 Artifacts；不自动覆盖正式 Release。

## 验证进度
- 前端类型检查/生产构建：通过首轮，最终修改后需复测。
- Rust HPP 测试：本机执行中。
- 插件完整流程、播放器与 UI 回归：待执行。
- macOS iOS 编译：待首次运行。
- IPA 结构、Mach-O、Info.plist、资源/权限：检查脚本已实现，待实际产物。
- iPhone 真机、原生横屏、PiP、长时间内存：未验证，没有连接设备。

## 下一步
完成本机回归，推送独立分支，运行并修复 GitHub Actions，下载校验实际产物，补充日志、下载地址和签名安装说明。

## 继续开发约定
每次继续前先读本文件、根目录 PROJECT_STATUS.md、当前源码及 git status/log。不得将浏览器模拟、构建成功等同于 iPhone 真机通过。
