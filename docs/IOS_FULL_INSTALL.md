# HGhub iOS 1.1.0 安装与验收

## 安装

1. 下载 HGhub_iOS_Full.ipa。保留文件原名，对照同目录 SHA256SUMS.txt 核验。
2. 用此前成功给 HGhub 签名的个人工具和 Apple 签名身份重新签名。若要保留旧版收藏与历史，继续使用同一应用标识和签名团队，直接覆盖安装。应用标识是 com.yann359.hghub.ios，签名工具也可能按个人账号附加后缀。
3. 安装签名后的 IPA。若 iPhone 对开发签名应用提示启用开发者模式，按 [Apple 开发者模式说明](https://developer.apple.com/documentation/xcode/enabling-developer-mode-on-a-device) 操作。
4. 联网打开 HGhub。首次启动可在「我的 → 插件中心」看到两个片源和一个解析器。试搜「大雄兔」或「月相」，进入作品详情并开始播放。

Windows 可以运行 Get-FileHash -Algorithm SHA256 .\HGhub_iOS_Full.ipa。macOS 可以运行 shasum -a 256 HGhub_iOS_Full.ipa。

## iPhone 真机验收

1. 用个人开发证书安装后，确认首次进入应用首页显示真实作品。退出并再次打开，确认插件和本地片库保留。
2. 搜索「大雄兔」，核对详情和海报，播放 Mux HLS。打开「画质与线路」，切换到 Internet Archive MP4，观察画面与播放进度。
3. 搜索「月相」，播放完成首帧后切到「月球轨道」并选择可用清晰度。搜索「辛特尔」确认英语字幕可选。试用倍速和系统全屏，查看顶部灵动岛及底栏的安全距离。
4. 停用 NASA 解析器，验证 Blender 仍能观看。重新启用解析器。测试 Files 文件导入时，将 JSON 清单和三个 JS 文件放在同一文件夹，使用多选一次选入。也可以在插件中心粘贴 [HTTPS 插件清单](https://raw.githubusercontent.com/yann359/HGhub/codex/ios-liquid-glass-plugin/bundled-plugins/manifest.json)。
5. 在测试设备确认收藏、观看历史和续播。全新安装验收可使用另一台测试设备或独立的签名应用标识，以保留旧应用的数据。

遇到播放失败时记录设备型号、iOS 版本、作品、分集、线路与错误文案，便于区分上游网络、解码和插件状态。个人签名凭据由签名工具保管。

## 复现构建

在 macOS 上安装 Xcode 和 Rust iOS arm64 目标。源码根目录执行 pnpm install --frozen-lockfile，再执行 pnpm tauri ios init --ci。执行 HGHUB_IOS_IPA_NAME=HGhub_iOS_Full.ipa bash scripts/build-ios-unsigned.sh 1.1.0。产物在 artifacts，包含 IPA、结构校验清单和 SHA-256 文件。

GitHub Actions 的 iOS 工作流使用同一脚本。供个人证书侧载的构建不会把配置文件或签名身份编入 IPA。测试、实际构建和签名状态见验收报告。
