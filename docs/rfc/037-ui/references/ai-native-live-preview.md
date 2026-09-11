# AI 原生 · 即时预览与所见即所得（Live Preview & WYSIWYG）

> 本子项承载 [037 §10(../../037-ui.md) 的 G1（所见即所得）/ G2（生成即见）/ G3（渲染面即对接面）。
> 配套子项：[render-capture](ai-native-render-capture.md) · [layout-snapshot](ai-native-layout-snapshot.md) ·
> [fidelity-loop](ai-native-fidelity-loop.md) · [multimodal-pipeline](ai-native-multimodal-pipeline.md) ·
> [visual-host](ai-native-visual-host.md)。

## 1. 目标

| # | 目标 | 判定 |
|---|------|------|
| G1 | 设计时与运行时**几乎一致**的呈现效果 | 同一 spec 在 LivePreviewHost 与 WindowHost 下的渲染结果一致（除宿主差异：窗口 chrome/输入栈） |
| G2 | 生成 ARML/spec **即见效果**，无需编译-运行循环 | 从 spec 到可见帧：无编译、无运行期宿主；属性补丁到新帧：单帧重渲染 |
| G3 | 渲染产物是一等公民 | 预览帧可写 PNG 供 LLM/CI 分析，也可作为纹理嵌入真实运行界面（VideoSurface 对接） |

## 2. 双宿主渲染原则（WYSIWYG 的架构保证）

**同一代码路径，双宿主**：LivePreviewHost 与 WindowHost 共享 LayoutManager + WgpuRender + RenderTree，
差异仅在宿主装配与输入栈。

| 面 | WindowHost（运行时） | LivePreviewHost（预览） |
|----|---------------------|------------------------|
| 渲染目标 | 窗口 surface | 离屏 target（不依赖 HWND，headless 可测） |
| 帧泵 | FramePump（事件 + 脏标记 + 插值帧） | 单帧按需渲染（无帧泵） |
| 输入栈 | FocusManager / KeyboardRouter / IME 全量 | 无（首版预览无交互） |
| 生命周期 | Window 窗口生命周期 | VisualHost 生命周期事件（InnerLoaded 等） |
| 资源/主题 | Application 唯一解析根 | VisualHost 帧内 ResourceDictionary（默认合并 Light） |

WYSIWYG 不是「两套渲染做对比」，而是**同一个渲染面换宿主**——这是「几乎一致」的架构级保证，也是单一惯用法（禁软件光栅 / HTML / WebView 预览后端）的落点。

## 3. LivePreviewHost 契约

    namespace Arc.UI.Components;

    /// <summary>即时预览宿主：无窗口离屏渲染 + 单帧重渲染 + 属性补丁。</summary>
    public class LivePreviewHost : VisualHost {
        /// <summary>以 spec/ARML 字符串构建内层树并渲染一帧（0 尺寸 = 沿用当前视口）。</summary>
        public ArmlParseResult LoadSpec(string arml, double width, double height);

        /// <summary>属性补丁：按元素路径改 DP 字符串值并单帧重渲染（改即见）。</summary>
        public bool ApplyPatch(string elementPath, string propertyName, string value);

        /// <summary>渲染当前内层树到离屏 target 并回读为 PNG 文件。</summary>
        public bool CapturePng(string filePath);

        /// <summary>当前内层树的结构化布局快照。</summary>
        public LayoutSnapshot GetLayoutSnapshot();

        /// <summary>重置为初始状态（卸载内层树）。</summary>
        public void Reset();
    }

要点：

| 面 | 决策 |
|----|------|
| 构建 | LoadSpec = 运行时 `ArmlParser` 解析实例化 → `SetContent` → 离屏单帧；**空/非法 spec 返回诊断且不建树**。编译期完整校验仍由 **arc-ui typeck** 承担（ARML 工程路径），本入口不二次跑 typeck |
| 补丁 | ApplyPatch：元素路径（`Root/Title` 或类型段）+ 属性名 + **字符串值**（经 `DpValueConverter`）→ 重布局 → **单帧渲染**。未知路径 / 未知 DP 返回 `false`，树不变 |
| 帧 | 无帧泵：每次 ApplyPatch / LoadSpec 后渲染一帧到离屏 target；截图按需回读 |
| 尺寸 | 预览尺寸由调用方指定（如 1280×800 或窗口当前尺寸）；自适应投影按该尺寸取环境快照 |
| 截图 | CapturePng 经 [render-capture](ai-native-render-capture.md) 回读编码；headless（无 display）同样可用 |
| 绘制 | 预览走 `TreeDrawListBuilder` → 同一 DrawList IR / `WgpuRender` 离屏面；**不是** WindowHost `RenderTree` 第二套渲染——G1 双宿主像素一致另排 |

## 4. 属性补丁语义（改即见）

- ApplyPatch 的语义是**确定性单帧重渲染**：旧帧 → 属性变更 → 布局重算 → 新帧。
- 补丁前后的两帧均可截图，形成 before/after 视觉对（LLM 对比分析、diff 用）。
- 补丁目标解析走**受限路径**（元素路径 + 显式属性表），禁止任意反射（对齐 037 §10 边界）。
- 性能预算：单帧渲染毫秒级；补丁频率受调用方控制（LLM 回合级，非热路径）。

## 5. VideoSurface 对接（G3 渲染面即对接面）

- 预览帧（离屏 target）与 VideoSurface 纹理**同源于纹理对象**：离屏帧可经既有 UploadTexture 通道
  作为纹理表面嵌入真实运行界面——设计时预览直接「贴」进运行时窗口的某个区域。
- 双通道：

| 通道 | 消费方 | 机制 |
|------|--------|------|
| PNG 文件 | LLM 多模态上下文 / CI 验收 | CapturePng（render-capture） |
| 纹理表面 | 真实窗口内嵌预览（VideoSurface） | 离屏帧 → UploadTexture → VideoSurface 显示 |

- 意义：预览、评审、运行效果**同一渲染面**，杜绝「预览一套、运行一套」；VideoSurface 系列的
  Attach/Detach/Invalidate 生命周期（见 [texture-surface](texture-surface.md)）原样复用。

## 6. 边界（本子项）

- **无交互**：LivePreviewHost 无输入路由 / 焦点 / IME；交互预览（点击/键入反馈）为后续能力
  （独立焦点域/输入路由/IME 隔离，见 [visual-host](ai-native-visual-host.md)）。
- 不替代编译期 ARML：运行时 `ArmlParser` 是实例化简化面（未知类型 fallback Element）；产品面 `.arml` 仍走 arc-ui typeck，非第二套语法。
- 截图不无条件进 LLM 上下文：渐进披露——先 [layout-snapshot](ai-native-layout-snapshot.md) 文本，必要时才截图。
- 动态绑定（DataContext 运行时路径）走受限通道，见 [visual-host](ai-native-visual-host.md) §4。
- **不**宣称 G1（LivePreviewHost 与 WindowHost 像素一致）、G3（预览帧贴 `VideoSurface`）、审视回路 / 像素闸。

## 7. 验收 checklist（G2 最小硬门槛）

> **宣称纪律**：下列仅关「spec 字符串 → 离屏单帧 + 属性补丁可 CI」；勾选后仍 **不** 宣称 G1 双宿主像素一致、G3 VideoSurface、运行时 arc-ui typeck、审视回路、像素闸或「AI 原生全部完成」。

| 项 | 状态 | 证据 |
|----|------|------|
| `Initialize` 离屏（无 HWND）+ `LoadSpec` 建树并单帧 | ✅ | `LivePreviewHost.as` |
| 空 spec：`Success=false` + 诊断 + 不建树 | ✅ | `g2_loadspec_empty_diag` |
| `ApplyPatch` 改 `TextBlock.Text`：属性值变 + 布局行盒变宽 | ✅ | `g2_apply_patch_text_layout` |
| 未知路径补丁返回 false，树不变 | ✅ | `g2_apply_patch_unknown_false` |
| `CapturePng`：文件存在 / PNG 魔数 / IHDR 尺寸 | ✅ | `g2_loadspec_capture_png`（经 `PngEncoder`） |
| `Reset` 清树：快照 null、补丁拒绝 | ✅ | `g2_reset_clears` |
| G1 双宿主像素一致 / G3 VideoSurface / 审视回路 | ☐ | 后置；本切片不宣称 |

验证：`cargo test -p arc-tests --features full-rt --test l2_ui_live_preview_g2_batch`。