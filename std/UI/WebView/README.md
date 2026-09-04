# Arc.UI.WebView

浏览器组件库：把**系统自带浏览器引擎**（Windows WebView2 / macOS WKWebView / Linux WebKitGTK）作为 wgpu 合成面内的一等纹理表面集成，并以前端 ↔ Arc 后端受控桥互联。引擎零打包。

**设计权威**：[RFC 037 references/webview-surface](../../../docs/rfc/037-ui/references/webview-surface.md)

**核心机制**（设计态，未实现）：

- **渲染面** `IBrowserSurface`：GPU 零拷贝捕获（wgpu-scry 方案）——Windows DXGI 共享纹理 → DX12、Linux DMA-BUF → Vulkan、macOS IOSurface → Metal，经 `IRender.ImportExternalTexture` 进 wgpu 合成面，`DrawTexture` 采样；
- **桥** `IBrowserBridge`：注入脚本 `__ARC_INTERNALS__` + 自定义协议（`asset://` / `ipc://`）+ `[WebCommand]` 编译期聚合 + 能力门闩（RFC 015）；
- **组件** `Arc.UI.Components.WebBrowser`：`VideoSurface` 同构生命周期（纹理归组件、引擎归系统）。

**状态**：实现未开工（单 M · 子集边界 · 不自动开干，见 实现规划）。本库当前仅注册命名空间与依赖拓扑。
