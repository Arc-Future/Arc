// RFC 037 D7 + RFC 037 §2.3 / §12: IRender — wgpu 渲染后端契约。
//
// 统一管线（RFC 037）：ARML RenderNode lowering 与 DrawContext 编码在
// DrawList IR 汇合，由 IRender 批绘。M-draw0 设计签收；
// M-draw1 落地 DrawList + DrawContext 类型与 DrawListExecutor。
//
// 渲染后端**唯一 = wgpu**（Arc.UI 只适配 wgpu，无 Software/Skia 双轨）：
//   - Windows → DirectX 12
//   - macOS   → Metal
//   - Linux   → Vulkan
//   - Web     → WebGPU / WebGL2 兜底
// 由 `WgpuRender`（std/UI/Rendering/Wgpu/）实现本接口。
//
// 设计原则（对齐 Arc 高性能章程 D7.2 + RFC 037 D7）：
//   1. 显式帧边界——BeginFrame/EndFrame 之间编码所有绘制命令
//   2. 零分配热路径——资源在 Initialize 一次性创建
//   3. GPU 优先——RenderPipeline + ShaderModule 走硬件加速
//   4. 平台无关——不暴露 HWND/Window/NSView
namespace Arc.UI.Rendering;

/// <summary>
/// 渲染后端契约（wgpu 唯一实现：<see cref="WgpuRender"/>）。
/// </summary>
public interface IRender {
    /// <summary>初始化渲染后端，绑定到原生窗口 handle。</summary>
    /// <returns>true 成功；false 失败。</returns>
    bool Initialize(long nativeWindowHandle, double width, double height);

    /// <summary>调整 Surface 尺寸（窗口 resize 时调用）。</summary>
    void Resize(double width, double height);

    /// <summary>开始一帧渲染。获取 surface texture 并 begin render pass。</summary>
    void BeginFrame(double width, double height);

    /// <summary>提交一帧渲染。End render pass + submit + present。</summary>
    void EndFrame();

    /// <summary>
    /// 渲染元素树到当前帧（M3 过渡路径；由 DrawList 取代后保留 wgpu 直读语义）。
    /// </summary>
    /// <param name="rootHandle">RtUiElement* 根节点句柄。</param>
    void RenderElementTree(long rootHandle);

    /// <summary>
    /// 批绘 DrawList IR（主路径；取代逐元素树遍历）。
    ///
    /// 须在 BeginFrame/EndFrame 之间调用。wgpu 即时解释 rect/line/text。
    /// </summary>
    /// <returns>0 成功；-1 未初始化或未实现；-2 含不支持命令。</returns>
    int ExecuteDrawList(DrawList list);

    /// <summary>
    /// P0 真实裁剪：推入一个裁剪矩形（scissor）。当前裁剪矩形与已有裁剪相交。
    /// 后续所有绘制仅落在裁剪矩形内；超出部分被硬件裁掉。
    /// 坐标用 surface 像素（左/上，y 向下），与 DrawRect 的 CSS 像素一致。
    /// 每个 PushClip 必须配对 PopClip。须在 BeginFrame/EndFrame 之间调用。
    /// </summary>
    void PushClip(double x, double y, double w, double h);

    /// <summary>P0 真实裁剪：弹出最近一次 PushClip 的裁剪矩形，恢复上一裁剪。</summary>
    void PopClip();

    // ===== 纹理表面契约（RFC 037 references/texture-surface）=====

    /// <summary>
    /// 创建动态纹理，返回纹理 id（0 失败）。usage 含 TEXTURE_BINDING + COPY_DST。
    /// 纹理内容由 <see cref="UploadTexture"/> 填充，供 DrawTexture 采样。
    /// </summary>
    int CreateTexture(int width, int height);

    /// <summary>
    /// 上传整幅像素到纹理（RGBA8，bytesPerRow = width*4）。须在 BeginFrame 之前调用。
    /// data 为像素缓冲 NativePtr（零拷贝透传）。
    /// </summary>
    void UploadTexture(int textureId, NativePtr data);

    /// <summary>销毁纹理并释放其视图。释放后 textureId 失效。</summary>
    void DestroyTexture(int textureId);

    // ===== AI 原生 · 渲染回读（RFC 037 §10 references/render-capture）=====

    /// <summary>
    /// 创建离屏渲染目标（RGBA8Unorm，RENDER_ATTACHMENT + COPY_SRC），返回 offscreenId（0 失败）。
    /// 离屏渲染不依赖窗口 surface，headless 可测。尺寸为物理像素，上限 2048×2048。
    /// </summary>
    int CreateOffscreenTarget(int width, int height);

    /// <summary>
    /// 离屏渲染一帧：以 offscreen 为 target 执行 DrawList（单帧语义，无帧泵）。
    /// 须在窗口帧之外调用（与 EndFrame 互斥）；width/height 与 target 尺寸一致（物理像素）。
    /// 返回 0 成功；-1 未初始化或 offscreen 无效；-2 与窗口帧交错；-3 离屏 pass 创建失败；-4 含不支持命令。
    /// </summary>
    int RenderToOffscreen(int offscreenId, DrawList list, double width, double height);

    /// <summary>
    /// 回读离屏像素为 RGBA8（须在 RenderToOffscreen 之后）。
    /// outRgba 为调用方缓冲句柄（long，rt_image_alloc 形态；容量 ≥ width*height*4）。
    /// long 形态避免 long→NativePtr cast（编译器缺陷 CD-29，转换在 C 层闭环）。成功 true。
    /// </summary>
    bool ReadbackPixels(int offscreenId, long outRgba, int capacity);

    /// <summary>销毁离屏目标。释放后 offscreenId 失效。</summary>
    void DestroyOffscreenTarget(int offscreenId);

    /// <summary>释放后端资源。</summary>
    void Shutdown();
}
