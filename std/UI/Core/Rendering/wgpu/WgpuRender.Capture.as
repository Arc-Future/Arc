// RFC 037 §10 AI 原生 AL-P0: WgpuRender —— 离屏渲染目标与像素回读（references/render-capture）。
//
// 离屏渲染 = 不依赖窗口 surface 的单帧渲染（headless 可测）：
//   - CreateOffscreenTarget：C 层 wgpu_offscreen_create（RGBA8Unorm + RENDER_ATTACHMENT|COPY_SRC）
//   - RenderToOffscreen：伪帧——帧准备（staging/命令记录/atlas flush）→ 离屏 render pass
//     → ExecuteDrawList 批绘 → FlushFrameCommands(false)（提交但不 present）
//   - ReadbackPixels：C 层 wgpu_offscreen_readback（copy_texture_to_buffer + map_async 同步）
// 单一惯用法：绘制命令走既有 DrawList 管线，与窗口帧同一渲染面，仅宿主不同（双宿主渲染雏形）。
//
// 离屏尺寸即物理像素（无 DPI 缩放，headless 语义）；上限 2048×2048（评审分辨率）。
// 离屏目标表用 List（offscreenId = index+1，销毁槽置 null）：评审目标数量极少，
// 避免 Dictionary.Keys 遍历的编译器缺陷（int_arr_get_Count 未定义，见 plan CD-29 登记）。
namespace Arc.UI.Rendering.Wgpu;

using Arc.Collections;
using Arc.UI.Rendering;

public partial class WgpuRender {
    // ===== AI 原生 · 渲染回读（RFC 037 §10 references/render-capture）=====

    /// <summary>
    /// 创建离屏渲染目标（RGBA8Unorm，RENDER_ATTACHMENT + COPY_SRC），返回 offscreenId（0 失败）。
    /// 离屏渲染不依赖窗口 surface，headless 可测。尺寸为物理像素，上限 2048×2048。
    /// </summary>
    public int CreateOffscreenTarget(int width, int height) {
        if (!_initialized || width <= 0 || height <= 0 || width > 2048 || height > 2048) {
            return 0;
        }
        NativePtr offscreen = wgpu_native.wgpu_offscreen_create(_device, width, height);
        if (offscreen == null) {
            return 0;
        }
        _offscreenTargets.Add(offscreen);
        return _offscreenTargets.Count;
    }

    /// <summary>
    /// 离屏渲染一帧：以 offscreen 为 target 执行 DrawList（单帧语义，无帧泵）。
    /// 须在窗口帧之外调用（与 EndFrame 互斥）；width/height 与 target 尺寸一致（物理像素）。
    /// 返回 0 成功；-1 未初始化或 offscreen 无效；-2 与窗口帧交错；-3 离屏 pass 创建失败；-4 含不支持命令。
    /// </summary>
    public int RenderToOffscreen(int offscreenId, DrawList list, double width, double height) {
        if (!_initialized || list == null) {
            return -1;
        }
        NativePtr offscreen = this.offscreenAt(offscreenId);
        if (offscreen == null) {
            return -1;
        }
        // 单帧语义：与窗口帧互斥（EndFrame 进行中禁止交错渲染）。
        if (_pass != null || _encoder != null) {
            return -2;
        }
        // 1. 帧准备（与 BeginFrame 同源：uniform offset 重置 + 命令记录清空 + staging 惰性创建 + atlas flush）。
        //    离屏尺寸即物理像素（dpiScale=1.0，DIP=物理像素）：scissor/裁剪以离屏尺寸为界。
        _uniformOffset = 0;
        _overflowDropped = 0;
        _dipWidth = (int)width;
        _dipHeight = (int)height;
        _surfaceWidth = (int)width;
        _surfaceHeight = (int)height;
        _cmdOffset.Clear();
        _cmdPipeline.Clear();
        _cmdScissorX.Clear();
        _cmdScissorY.Clear();
        _cmdScissorW.Clear();
        _cmdScissorH.Clear();
        _lastPipeline = -1;
        _lastScissorIdx = -1;
        if (_staging == null) {
            _staging = wgpu_native.wgpu_batch_staging_create(UniformBufferSize);
        }
        if (!_fontFallback && _fontAtlas != null) {
            wgpu_native.wgpu_font_atlas_flush(_fontAtlas, _queue);
        }
        // 2. 离屏 render pass（clear 黑色；离屏尺寸即物理像素，无 DPI 缩放）。
        _encoder = wgpu_native.wgpu_command_encoder_create(_device);
        _pass = wgpu_native.wgpu_offscreen_begin_pass(
            offscreen, _encoder, 1, 0.0, 0.0, 0.0, 1.0
        );
        if (_pass == null) {
            wgpu_native.wgpu_release(_encoder);
            _encoder = null;
            return -3;
        }
        // 3. 批绘 DrawList（既有 ExecuteDrawList 主路径；不支持命令映射为 -4）。
        int rc = this.ExecuteDrawList(list);
        // 4. 提交（不 present；offscreen 纹理与视图由离屏目标持有，不在本帧释放）。
        this.FlushFrameCommands(false);
        if (rc == -2) {
            return -4;
        }
        return 0;
    }

    /// <summary>
    /// 回读离屏像素为 RGBA8（须在 RenderToOffscreen 之后）。
    /// outRgba 为调用方缓冲句柄（long，rt_image_alloc 形态；容量 ≥ width*height*4）。
    /// long 形态避免 long→NativePtr cast（编译器缺陷 CD-29，转换在 C 层闭环）。成功 true。
    /// </summary>
    public bool ReadbackPixels(int offscreenId, long outRgba, int capacity) {
        if (!_initialized || outRgba == 0) {
            return false;
        }
        NativePtr offscreen = this.offscreenAt(offscreenId);
        if (offscreen == null) {
            return false;
        }
        int rc = wgpu_native.wgpu_offscreen_readback(offscreen, _queue, outRgba, capacity);
        return rc == 0;
    }

    /// <summary>销毁离屏目标。释放后 offscreenId 失效。</summary>
    public void DestroyOffscreenTarget(int offscreenId) {
        if (!_initialized) {
            return;
        }
        if (offscreenId < 1 || offscreenId > _offscreenTargets.Count) {
            return;
        }
        NativePtr offscreen = _offscreenTargets[offscreenId - 1];
        if (offscreen == null) {
            return;
        }
        wgpu_native.wgpu_offscreen_destroy(offscreen);
        _offscreenTargets[offscreenId - 1] = null;
    }

    /// <summary>按 offscreenId 取离屏句柄（index+1 映射；越界/已销毁返回 null）。</summary>
    private NativePtr offscreenAt(int offscreenId) {
        if (offscreenId < 1 || offscreenId > _offscreenTargets.Count) {
            return null;
        }
        return _offscreenTargets[offscreenId - 1];
    }
}
