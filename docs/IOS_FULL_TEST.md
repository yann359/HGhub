# HGhub iOS 1.1.0 验收报告

验证日期 2026-11-24（北京时间）。应用源码构建基准 98cedd3，合入上游 v0.0.5。文件签名、模拟器与实体设备结果独立记录。

| 验收项 | 当前证据 | iPhone 真机 |
| --- | --- | --- |
| 全新启动与默认插件初始化 | Windows 真实 Rust 宿主首次创建空目录，三个内置模块均被安装并初始化为 ready | 待验 |
| 首页、分类、聚合搜索、作品详情 | 两组真实目录经 HPP 返回，iPhone 视口的 Edge 实测展示首页并从搜索打开详情 | 待验 |
| 播放真实授权内容 | Edge 解码并推进《大雄兔》HLS 和 NASA 月相 MP4，经 Rust 媒体代理 | 待验 |
| 切换线路、分集、清晰度 | Edge 实测《大雄兔》切到备用 MP4、NASA 切「月球轨道」后继续播放。宿主经 NASA API 选择 720p 并取得真实 HTTP 206 | 待验 |
| 字幕、全屏、倍速 | 《辛特尔》官方 SRT 经代理转为 VTT 实测。全屏和倍速通过合成素材的桌面与安卓视口回归 | 系统全屏、字幕轨与倍速待验 |
| 收藏、观看历史、续播、追剧 | 合成素材的桌面浏览器端到端验证收藏和观看历史。继续播放、进度和追更相关存储由 HPP 回归覆盖；片源更新数据按实际能力提供 | 待验 |
| 重新启动、故障隔离与恢复 | 临时数据目录重开后保留停用与卸载。停用 NASA 解析器后 Blender 仍可播放。三次运行时中断立即进入失败，手动重试专项通过 | 待验 |
| 本地/HTTPS 清单与更新 | 本地清单身份版本、HTTPS 预览及订阅、远端版本检查通过。HPP 回归覆盖升级、失败恢复、自动安装策略和并发行为 | iOS Files 原生选择待验 |
| 无开发电脑服务 | 预置模块编入应用，架构只依赖外网片源和应用内本机媒体代理。浏览器测试使用开发桥，iOS 结果见下方 | 待验 |
| 包结构与个人签名 | Xcode 27.1 的 arm64 Release IPA 已通过云端和本机两次结构与 SHA-256 校验 | 无可用的签名证书、描述文件及已连接的真机，未执行安装 |

## 执行环境

- Windows 原生 Rust 应用检查、前端类型检查及生产构建通过。Vite 提示主 JS 包超过 500 kB。
- Windows 与 Xcode 27.1 的 macOS 构建环境均通过 97 项 HPP Rust 回归。包含真实 QuickJS 与模拟 HTTP/媒体、导入和更新、限流、运行时预算、故障处理、源与解析器分层选择、MP4/HLS、字幕及本地片库。
- Windows Edge 用真实授权上游通过 10 组检查。另通过真实 HPP 代理和 ffprobe 核验 NASA 7 个分集以及《辛特尔》的 14 个 MP4 变体，全部为 H.264。Apple 硬件、WKWebView、iOS 系统文件选择器由各自环境验收。
- Windows Edge 的合成素材播放器专项通过 3 组。验证 Windows 桌面与 Android 视口的播放交互。上游旧版整合脚本仍有“点击搜索结果后直接播放”的断言，与新版先打开详情的入口不一致，本次未标为通过。
- 官方 HPP 文档构建及技能包安装检查通过。GitHub Pages 工作流调度返回 404，交付文档随完整源码提供。

## iOS 结果

真机架构 IPA 已由 GitHub Actions 的 Xcode 27.1 编译并验包。相同应用源码的 Xcode 27.1 arm64 模拟器编译也通过。iPhone 18 Pro Max / iOS 27.2 的 CoreSimulator 等待系统启动在 150 秒后超时，关闭命令同样超时。测试尚未进入 App 安装或启动，模拟器运行、本机 IPC 和原生播放未得到验证。[模拟器运行记录](https://github.com/yann359/HGhub/actions/runs/45386761668)

## IPA 文件

- HGhub_iOS_Full.ipa，5,591,120 字节。SHA-256 `34b83b0af255492fdff148c9de9a7940505672784a9a20c3d195db7444a7ff82`。
- 已核对 Payload/HGhub.app/Info.plist、arm64 设备 Mach-O、最低 iOS 16.4、未加密和未签名状态、ATS 本机网络设置、完整清单与三个插件源。
- GitHub Actions 和 Windows 本地 SHA-256 一致。[构建记录](https://github.com/yann359/HGhub/actions/runs/45384124235) · [下载产物](https://github.com/yann359/HGhub/actions/runs/45384124235/artifacts/14915483439)

## 检查入口

- 实际上游和播放测试脚本在 scripts/test-bundled-sources.mjs。执行前用 cargo build --locked --manifest-path src-tauri/crates/hpp/Cargo.toml --example dev_host 构建 QuickJS 宿主。
- 合成素材播放器专项使用 node scripts/test-plugins.mjs --experience。
- 手机操作步骤见 [安装说明](IOS_FULL_INSTALL.md)，详细授权见 [片源报告](IOS_FULL_SOURCES.md)。
