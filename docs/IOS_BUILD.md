# HGhub iPhone 定制版

仅用于非商业学习与个人使用，遵守根目录 PolyForm Noncommercial 1.0.0 许可证与原版权声明。应用不附带第三方插件、账号或影视资源。

## 源码与支持范围

开发分支为 `codex/ios-liquid-glass-plugin`，基于本仓库插件版 `2379957`。上游 Minireel/HGhub 已撤下源码和 Release；本地旧版 v1.2.1 缺少插件架构，因此没有把它冒称为当前插件版的稳定版本。Windows、Android 使用原有构建配置，iOS 配置单独覆盖版本、Bundle ID 和构建命令。

- React + Tauri 2、Rust HPP/QuickJS 插件宿主与本机媒体代理，运行实际插件与媒体逻辑。
- 玻璃视觉使用 WebKit 半透明与背景模糊，是 Web UI；并非 SwiftUI 原生 Liquid Glass。
- iOS 最低 16.4：项目已有 Tailwind CSS 4 的官方 Safari 支持下限为 16.4，插件选择器也依赖 `dialog.showModal()`。采用此下限避免旧 WebKit 出现颜色、布局与弹窗不完整。
- iOS Bundle ID：`com.yann359.hghub.ios`；发行版本：`1.0.0`。
- iOS 优先 WebKit 原生 HLS/MP4；支持范围取决于系统解码器、源编码和合法访问条件。其他平台保留 hls.js。
- 全屏与画中画调用真实系统暴露的媒体 API；系统不支持时显示提示。应用进入后台时暂停，前台仅恢复此前正在播放的视频；画中画按系统支持处理。
- 未添加后台音频权限或任意网络访问例外。Rust 代理仍只监听 `127.0.0.1`，通过随机媒体会话标识隔离资源；ATS 只声明本机网络。插件网络权限、凭据与取消机制保留。

## 在 GitHub Actions 构建

1. 打开仓库 **Actions → HGhub iOS Liquid Glass**，选择分支 `codex/ios-liquid-glass-plugin`，点击 **Run workflow**。该分支代码变更也会自动构建。
2. 等待 macOS runner 完成 Rust 测试、iOS 初始化和 ARM64 Release 编译。
3. 成功后在运行页面底部 **Artifacts** 下载 `HGhub-iOS-LiquidGlass-v1.0.0`。解压得到 `.ipa`、`ios-build-manifest.json` 和 `SHA256SUMS.txt`。Artifacts 会按工作流配置保留 30 天，下载需要登录 GitHub。
4. `HGhub-iOS-build-logs` 包含构建环境、前端、Rust 测试、Tauri、Xcode 完整日志及可用的 xcresult。失败时同样上传诊断。

工作流不需要 Apple Team ID、证书或 Provisioning Profile，因为此流程只生成待个人重新签名的安装包。脚本保留 Tauri Release 参数服务器，并让 Xcode 对真机目标构建，禁用代码签名，最终将实际 `.app` 放入 `Payload/` 后打包。

## 在 Mac 本地构建

安装 Xcode 与命令行工具、Rust stable、Node 22、pnpm 10.34.9，再执行：

```sh
git checkout codex/ios-liquid-glass-plugin
pnpm install --frozen-lockfile
rustup target add aarch64-apple-ios
pnpm tauri ios init --ci
bash scripts/build-ios-unsigned.sh 1.0.0
```

输出位于 `artifacts/`。Windows 可开发与回归测试，但无法本地执行 Xcode 编译。

## 验证安装包

```sh
python3 scripts/verify-ipa.py artifacts/HGhub-iOS-LiquidGlass-v1.0.0.ipa \
  --bundle-id com.yann359.hghub.ios --version 1.0.0 \
  --manifest artifacts/ios-build-manifest.json
```

检查 ZIP CRC、唯一 `Payload/*.app`、Info.plist、最低系统、Bundle ID、横竖屏声明、可执行权限、ARM64 Mach-O、真机平台、PIE、加密与签名状态、开发服务器地址。Release 编译参数及 `custom-protocol` 特性由 Xcode/Rust 完整日志提供证据；结构检查本身无法证明所有运行行为。`deviceTested` 始终为 `false`，直到实际设备验收并另行记录。

## 个人签名与安装

**未签名 IPA 不能直接安装。** 可以交给支持个人签名的工具重新签名，使用你自己的 Apple ID／开发证书、匹配的 App ID 与设备 Provisioning Profile。不要使用他人的证书或虚假 Team ID；证书、密码和描述文件不要提交到仓库。

1. 下载 IPA 并与 SHA256SUMS 对照，保留一份原文件。
2. 在你选用的个人签名工具中导入 IPA，按工具的流程连接 iPhone、选择自己的签名身份并安装。工具可能将 Bundle ID 改为你的唯一标识。
3. 按设备提示完成信任／开发者模式设置；具体要求与签名有效期取决于系统和 Apple 账号类型。
4. 首次打开从 **我的 → 插件中心** 导入你有权使用的插件，再验证浏览、播放与片库。

若需由 Xcode 直接生成已签名包，需要真实开发团队、有效证书私钥及匹配当前 Bundle ID／设备的 Provisioning Profile，再选择合适导出方式。此仓库不保存这些私有材料。

## 测试与真机验收

本地测试命令：

```sh
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/crates/hpp/Cargo.toml --locked
cargo build --manifest-path src-tauri/crates/hpp/Cargo.toml --example dev_host --locked
node scripts/test-plugins.mjs
node scripts/test-plugins.mjs --frontend
```

浏览器测试通过真实 Rust/QuickJS 宿主及媒体代理，只替换 Tauri IPC 传输。使用原创合成视频和虚构插件，覆盖实际 MP4/HLS 解码、插件管理与业务流程。iPhone 尺寸与 UA 测试仍运行在 Chromium，不能代替 WKWebView 真机验证。

真机仍需逐项验收：

- 首次安装与冷启动、无插件空状态；本地文件选择与插件导入／更新／禁用。
- HTTPS MP4、HLS 主清单／分段／密钥、鉴权来源、清晰度／线路／选集和反复拖动。
- 单击控制栏、双击暂停恢复；横屏原生全屏及退出后的版面；支持设备上的 PiP。
- 锁屏、来电／音频中断、后台切回、耳机切换；系统拒绝自动播放后手动播放。
- 浅色／深色／随系统、大号动态字体、VoiceOver、减少动态效果；刘海、灵动岛与 Home Indicator。
- 持续播放至少 60 分钟，用 Xcode Instruments 观察内存；多次切集与离开播放器后确认会话释放。

已通过和未通过的实际结果、构建地址及遗留风险以根目录 `PROJECT_STATUS.md` 为准。

## 官方依据

- [Apple HIG：材质](https://developer.apple.com/design/human-interface-guidelines/materials)、[布局](https://developer.apple.com/design/human-interface-guidelines/layout)、[无障碍](https://developer.apple.com/design/human-interface-guidelines/accessibility)
- [Tauri 2 iOS 前置环境](https://v2.tauri.app/start/prerequisites/)、[CLI](https://v2.tauri.app/reference/cli/)
- [WebKit iOS 视频策略](https://webkit.org/blog/6784/new-video-policies-for-ios/)
- [Tauri 2.10.1 参数服务器源码](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.10.1/crates/tauri-cli/src/mobile/mod.rs)

- [Tailwind CSS 4 浏览器兼容范围](https://tailwindcss.com/docs/compatibility)
