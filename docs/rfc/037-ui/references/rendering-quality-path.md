# Arc.UI 渲染画质提升路径（wgpu 极致画质与流畅度）

> 本文是 [037 UI 声明式框架(../../037-ui.md) 的**渐进式披露子项**。沉淀对 Arc.UI 当前 wgpu 渲染架构的实证调研与业界顶级方案对标结论，作为画质/流畅度提升的**能力路径**。**未经验收协议不得宣称画质达标**（宣称纪律）。
>
> 关联：渲染后端架构见 037 §4；字体最小面见 [custom-fonts](custom-fonts.md)；生产字体门禁见 [production-surface](production-surface.md) §2。

## 1. 现状基线（实证）

对 `std/UI/Core/Rendering/wgpu/WgpuRender.*` 与 `crates/runtime-ui/rt_wgpu_native.c` 逐环节核对：

| 维度 | 现状 | 证据（crates/runtime-ui/rt_wgpu_native.c） |
|------|------|--------------------------------------------|
| Present 模式 | `Fifo`（VSync 阻塞） | `cfg.presentMode = (WGPUPresentMode)present_mode` 默认 1=Fifo |
| MSAA | **无**（`sampleCount=1`） | 两处 render pipeline `desc.sampleCount = 1` |
| 批处理 | CPU staging 连续缓冲 + 逐绘制 draw（动态 uniform 偏移重放） | `wgpu_batch_staging_create` / `wgpu_batch_rect_write` / `wgpu_batch_text_write` |
| 顶点数据 | 无 vertex/index buffer，`vertex_index` 内联生成单位 quad | WGSL 内联 |
| 重绘触发 | 事件驱动（`InvalidateRect` → `WM_PAINT`），按需渲染 | `platform/windows/*_win32.c` |
| 字体光栅化 | 单一 32px 基底 CPU 光栅化 → atlas → shader 采样 | `rt_font.c` + `WgpuRender.Wgsl.as` |
| 色彩空间 | sRGB surface（`BGRA8UnormSrgb`）+ sRGB→linear 混合 | `WgpuRender.Draw.as::SrgbToLinear`（类型化 `Color` 消费） |
| 采样器 | 字体 atlas 用 Linear 过滤 | `wgpu_sampler_create` 参数化 |
| 剪裁 | **无 scissor**，靠 CPU 剔除 | 全库无 `set_scissor_rect` |

**结论**：色彩链路（sRGB + linear 混合）已对齐业界正确水位；主要差距集中在**文本采样质量**（单一基底缩放光栅化）、**几何抗锯齿**（无 MSAA）、**提交效率**（逐绘制 drawcall）与**剪裁能力**。

## 2. 业界分层认知（关键）

文本渲染**没有单一银弹**，顶级方案按场景分层：

| 场景 | 最优技术 | 参考实现 |
|------|---------|---------|
| UI 正文（10–20px 静态） | 按物理像素**逐字号光栅化 + hinting + 灰度 AA** | Chromium/Skia、iced（glyphon，基于 cosmic-text，取代 wgpu_glyph） |
| 正文锐度上限 | **LCD 子像素渲染**（ClearType 级） | Chromium/Skia/DirectWrite——Rust 生态目前无人完成 |
| 大字号 / 缩放动画 | **MSDF**（多通道 SDF，RGB 取 median，保尖角） | Godot、Figma |
| 任意矢量 / 无限缩放 | GPU compute 光栅化（tile-based + prefix-sum） | Vello（13 阶段 compute 管线） |

> **关键纠偏**：单通道 SDF 对正文实为**负优化**——软化笔画、改变字重感知。Chromium / iced 正文不用 SDF。凡退化到「正文发虚」的路径不得视为提升。

## 3. 提升路径（按投入产出排序）

### P0 — 双轨文本：正文回归逐字号位图，特效保留 SDF

- **正文轨**：废弃单一 32px 基底缩放采样，改为 **per-size bucket**——按 `round(fontSize × dpiScale)` 分桶（16/18/24px 等各自独立光栅化），atlas 键扩展为 `(font, codepoint, sizeBucket)`，quad 像素对齐。正文锐度**追平 Chromium**。
- **特效轨**：SDF 保留，仅用于 >32px 标题、缩放动画等场景。
- 中期升级：**MSDF**（RGB 三通道距离场取 median）替代单通道 SDF，保住尖角（msdfgen 算法可嵌入 C runtime）。
- 关联：兑现 production-surface §2「atlas 文本采样与覆盖 AA 达到可读生产质感」。

### P0b — LCD 子像素渲染（超越业界的锐度杀手锏）

- 利用 wgpu **`DUAL_SOURCE_BLENDING`** feature：fragment shader 输出双色（颜色 + coverage mask），实现 Skia/DirectWrite 式 LCD filtering。
- 前提：glyph 光栅化横向 3 倍过采样（`stbtt_GetCodepointBitmapSubpixel` 横向 scale×3）。
- 效果：正文锐度**超越 egui/iced/Slint 全部 Rust UI 框架**（它们均为灰度 AA），达 Windows 原生 ClearType 观感。wgpu-native 支持该 feature，是 WebGPU 浏览器方案做不到的深度。

### P1 — MSAA 4x（几何抗锯齿）

- 所有 render pipeline `sampleCount=4` + resolve texture。当前 shader SDF AA 只处理图元内部，quad 几何边缘在非整变换下仍有锯齿；MSAA4 几何 AA + 内部 AA = Vello 同款双轨。

### P3 — Instancing（流畅度最大单项提升）

- 现状一屏 500 字形 = 500 个 drawcall + 500 次动态偏移切换。改为字形 quad 数据（pos/uv/color，~48B/字形）写 **instance buffer**，单次 `draw(6, N)` 画完全部文字；rect/shadow 同理。
- drawcall 从 O(N) → **O(1)**，滚动/动画流畅度质变，为 WebRender/epaint 标准做法。

### P4 — Scissor 剪裁栈

- `set_scissor_rect` 按绘制段设置，支持嵌套 ScrollView 正确剪裁；是后续 dirty-rect 局部重绘的前提。

### P6 — Vello 式 compute 光栅化

- 贝塞尔轮廓进 storage buffer，compute shader flatten/binning/fine 全 GPU 管线；零 atlas、任意变换零重光栅化。复杂度高，不在当前能力面（列为远期备选路径）；但这是 wgpu 赋予的天花板，Arc 自研编译器生成 WGSL 的能力是独特优势。

## 4. 非目标（能力边界）

- pack URI、HarfBuzz 整形、彩色 emoji、`FontStyle`（仍守 [custom-fonts](custom-fonts.md) 非目标）——**不得**借「健全」偷渡；
- Vello 式 compute 管线的**完整**形态（P6 为远期备选路径，仅允许渐进侦察）。

---

[返回 037 主题入口(../../037-ui.md) · [references 索引](index.md)
