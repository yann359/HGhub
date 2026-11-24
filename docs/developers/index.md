# HGhub 插件开发

把内容接入播放器，从一个 JavaScript 文件开始。

HGhub 管理界面、播放、网络、隔离与本地片库；插件负责站点数据。应用不内置内容源，也没有官方内容源列表。

## 从这里开始

1. [制作第一个插件](quickstart.md)：导入、搜索、详情与播放。
2. [能力与宿主 API](host-api.md)：根据站点选择能力，处理请求、存储和取消。
3. [调试与验收](testing.md)：从假宿主到真实 QuickJS 和播放器。
4. [导入、列表与更新](distribution.md)：单文件、列表、版本与订阅。
5. [让 Agent 协助开发](agent.md)：安装公开技能包与复制提示词。

当前应用 **0.0.5 / HPP 1.0.0-dev**。协议为草案，开发前阅读[当前支持状态](status.md)。

[协议总览](../plugin-protocol/README.md) · [完整宿主 API](../plugin-protocol/04-host-api.md) · [内容模型](../plugin-protocol/05-content-model.md) · [本地构建与部署](deployment.md)

网站不发送分析数据、不加载第三方字体。下载内容只有公开协议、类型和虚构示例，不包含本机内容源或账号。
