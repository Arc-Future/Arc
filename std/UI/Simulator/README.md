# Arc.UI.Simulator

模拟器承载组件库：模拟器界面集成（Arc.UI 研发版图成员）。

**定位**：承载模拟器 guest 界面——如基于 **Android 模拟器核心**的屏幕集成。渲染落点复用 Arc.UI **纹理表面**（[VideoSurface + DrawTexture](../../../docs/rfc/037-ui/references/texture-surface.md)）承载 guest 帧缓冲；输入注入/音频/内核集成为后续子项。

**状态**：实现未开工（单 M · 子集边界 · 不自动开干，见 实现规划）。本库当前仅注册命名空间与依赖拓扑。
