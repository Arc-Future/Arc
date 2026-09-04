# Arc.UI.WebWindow

效率开发框架（**未来规划**）：类似 Tauri 的桌面应用框架——系统 WebView 为应用 UI 表面（Arc.UI.WebView），Arc.Web 为后端 Web 框架，经注入脚本 + 自定义协议 + `[WebCommand]` 编译期聚合 + 能力门闩（RFC 015）互联。

**设计权威**：[RFC 037 references/webview-surface](../../../docs/rfc/037-ui/references/webview-surface.md)（WebView 表面与桥为基座，本库为装配层）

**依赖拓扑**：`Arc.UI.WebWindow` → `Arc.UI.WebView` + `Arc.UI` + `Arc.Web`

**状态**：实现未开工（单 M · 子集边界 · 不自动开干，见 实现规划）。本库当前仅注册命名空间与依赖拓扑。
