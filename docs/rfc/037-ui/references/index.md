# Arc.UI · 渐进式披露子项（references）

> 本目录承载 [037 UI 声明式框架(../../037-ui.md) 的**能力子项**。037 主文档保留架构级表述；深度设计、契约细节下沉至此，按需钻取。**一子项一文档，互不重叠**；子项仅补细节，不与主文档重复表述既有决策。

| 子项 | 内容 | 关联主文档章节 |
|------|------|---------------|
| [纹理表面（VideoSurface + DrawTexture）](texture-surface.md) | 动态帧缓冲作为 UI 表面；`DrawTexture` 绘制原语与纹理生命周期契约 | 037 §4 渲染与虚拟化 |
| [WebView 表面与桥（WebBrowser 组件）](webview-surface.md) | 系统引擎零拷贝捕获进 wgpu 合成面；`IBrowserSurface`/`IBrowserBridge` 双面架构；注入脚本 + 自定义协议 + `[WebCommand]` 聚合 + 能力门闩；平台天花板 | 037 §4 §6–§8 |
| [自定义字体（FontManager + markup）](custom-fonts.md) | 命名族注册、项目相对路径、FontFamily/FontSize/FontWeight 最小面；pack/HarfBuzz/emoji 非目标 | 037 §9 自定义字体 |
| [内置主题资源（ResourceDictionary.arml）](builtin-theme-resources.md) | Light/Dark 色值与控件隐式 Style 的 ARML 正道；几何/motion 留 AS；禁色值双源 | 037 §4 主题 |
| [生产面契约（分层 · 对齐 · 滚动条）](production-surface.md) | 每层能力闭合；字体生产门禁；H/V 与 ContentAlignment；竖滚动条可见性/交互/样式 | 037 §4–§5 · §8 |
| [渲染画质提升路径（wgpu 极致画质与流畅度）](rendering-quality-path.md) | 实证基线；正文逐字号位图 + LCD 子像素 + MSDF + MSAA + Instancing 能力路径 | 037 §4 · production-surface §2 |
| [文本编辑契约（TextBoxModel · 改名清单）](text-editing.md) | TextBoxModel 内核契约；TextBlock/TextBox 命名修订与改名清单；输入栈缺陷定案 | 037 §8 |
| [AI 原生 · 即时预览与所见即所得](ai-native-live-preview.md) | 双宿主渲染（同一代码路径）、LivePreviewHost、属性补丁（改即见）、VideoSurface 对接 | 037 §10 · G1/G2/G3 |
| [AI 原生 · 渲染回读契约](ai-native-render-capture.md) | 离屏 target、RenderToOffscreen、ReadbackPixels、PngEncoder；headless 可测 | 037 §10 §4 |
| [AI 原生 · 布局快照契约](ai-native-layout-snapshot.md) | GetLayoutSnapshot、结构化布局树 + 文本行盒、确定性、JSON | 037 §10 §5 |
| [AI 原生 · 保真闭环](ai-native-fidelity-loop.md) | DesignTokenCatalog + 无裸值校验、组件 Golden、审视回路、三层验收协议、人审固化 | 037 §10 §6–§10 |
| [AI 原生 · 多模态输入管线](ai-native-multimodal-pipeline.md) | 图 → 版面解析 → 组件识别 → spec → 校验 → 渲染审视迭代 | 037 §10 多模态 |
| [AI 原生 · VisualHost 演进](ai-native-visual-host.md) | 三角色正式化（生成容器/评审单元/预览宿主）、ApplyPatch、动态绑定受限通道、交互预览后续能力 | 037 §10 §11 |

---

[返回 037 主题入口(../../037-ui.md) · [返回 RFC 索引](../../index.md)
