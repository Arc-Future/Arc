# AI 原生 · 渲染回读契约（RenderCapture）

> 本子项承载 [037 §10(../../037-ui.md) 感知基础设施的「眼睛」：离屏帧回读为 CPU 像素并编码 PNG。
> 配套：[live-preview](ai-native-live-preview.md)（消费方）· [layout-snapshot](ai-native-layout-snapshot.md)（尺子）· [fidelity-loop](ai-native-fidelity-loop.md)（审视回路）。

## 1. 目标

让 Arc.UI 的渲染产物（离屏帧）成为一等公民：可回读为 RGBA 像素、可编码为 PNG 文件，
供 LLM 多模态上下文、CI 验收与设计时预览消费。**无窗口（headless）可测**是本契约的硬要求。

## 2. 契约

    namespace Arc.UI.Rendering;

    public interface IRender {
        // ... 既有成员（Initialize/Resize/BeginFrame/EndFrame/ExecuteDrawList/PushClip/...）

        /// <summary>创建离屏渲染目标（RENDER_ATTACHMENT + COPY_SRC），返回 textureId（0 失败）。</summary>
        int CreateOffscreenTarget(int width, int height);

        /// <summary>离屏渲染一帧：以 offscreen 为当前目标执行 BeginFrame/DrawList/EndFrame。</summary>
        bool RenderToOffscreen(int textureId, DrawList list, double width, double height);

        /// <summary>回读离屏 target 像素为 RGBA8（阻塞至 fence 完成；须在 RenderToOffscreen 之后）。</summary>
        bool ReadbackPixels(int textureId, NativePtr outRgba, int capacity);

        /// <summary>销毁离屏 target。释放后 textureId 失效。</summary>
        void DestroyOffscreenTarget(int textureId);
    }

PNG 编码（std 层）：

    namespace Arc.UI.Rendering;

    /// <summary>RGBA8 像素 → PNG 文件（无外部依赖；复用 Arc.Drawing 编码能力，禁双轨）。</summary>
    public static class PngEncoder {
        public static bool Encode(string filePath, NativePtr rgba, int width, int height);
    }

## 3. 技术路径

| 环节 | 机制 |
|------|------|
| 离屏 target | offscreen texture（usage = RENDER_ATTACHMENT | COPY_SRC），非 surface：不依赖 HWND，headless 可测 |
| 渲染 | RenderToOffscreen：以离屏 target 为 color attachment 执行一次 DrawList（与窗口帧同一 DrawList 管线，单一惯用法） |
| 回读 | copy_texture_to_buffer → buffer map_async → fence 同步 → CPU RGBA8 |
| 编码 | PngEncoder：RGBA8 → PNG（行序自顶向下，y 翻转按契约固定） |
| 异步 | 回读是异步/低频操作（评审回合级）；ReadbackPixels 同步出口 = 一次 fence 等待，明确不进热路径 |
| ABI | rt_wgpu_native.c 增 offscreen/readback 面（webgpu.h 已含 wgpuBufferMapAsync / WGPUMapAsyncStatus 原语） |

## 4. 生命周期与诚实边界

- textureId 所有权：创建即调用方所有，DestroyOffscreenTarget 幂等释放（对齐 CreateTexture/DestroyTexture 先例，见 [texture-surface](texture-surface.md)）。
- 分辨率上限：评审分辨率（默认 ≤ 2048×2048），防无界显存/回读带宽；超限创建失败（显式错误，不静默降级）。
- 回读失败（设备丢失/尺寸不符）→ 返回 false + 显式告警，禁止静默返回空帧。
- 与窗口 surface 渲染互斥：RenderToOffscreen 与 EndFrame 不得交错（单帧语义，宿主负责调度）。