// RFC 037 references/texture-surface: Arc.UI.Components — VideoSurface 纹理表面组件。
//
// 承载「持续刷新的动态帧缓冲」作为 UI 表面（Android 模拟器屏幕、视频播放、
// 地图、摄像头预览等）。首版仅「显示」：把后端创建的动态纹理采样绘制到本元素矩形。
// 输入注入 / 音频 / 模拟器内核集成列为第二期（见 references/texture-surface.md §3.3）。
//
// 生命周期所有权：纹理由**消费方**（持有 IRender 的宿主，如模拟器）经
// Attach/UploadFrame/Detach 管理；本组件仅持有 TextureId 驱动渲染，不自行造纹理。
// 单动态纹理槽（后端 CreateTexture 单槽，见 WgpuRender）。

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Internal;
using Arc.UI.Rendering;

/// <summary>纹理表面组件——把动态纹理采样绘制到元素矩形（首版仅显示）。</summary>
public class VideoSurface : Control {
    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>TextureId 属性元数据——后端动态纹理 id（0 = 无纹理）。</summary>
    public static DependencyProperty<int> TextureIdProperty =
        RegisterProperty<int>(nameof(TextureId), typeof(VideoSurface), 0);

    /// <summary>Stretch 属性元数据——缩放模式，默认 Stretch.None。</summary>
    public static DependencyProperty<Stretch> StretchProperty =
        RegisterProperty<Stretch>(nameof(Stretch), typeof(VideoSurface), Stretch.None);

    /// <summary>当前绑定后端（Attach 持有，Detach/卸载释放）——卸载销毁纹理用。</summary>
    private IRender _backend;

    /// <summary>是否已登记卸载退订（防重复登记）。</summary>
    private bool _detachRegistered;

    public VideoSurface() {
        this.Type = typeof(VideoSurface);
        this.TypeName = "VideoSurface";
        this.SurfaceUpdated = new Signal<bool>(false);
    }

    /// <summary>帧更新信号——每次 <see cref="UploadFrame"/> 后触发（宿主可按需挂接）。</summary>
    public Signal<bool> SurfaceUpdated;

    /// <summary>后端动态纹理 id（0 = 未绑定）。由 Attach 赋值，Detach 清零。</summary>
    public int TextureId {
        get { return this.GetValue<int>(TextureIdProperty); }
        set { this.SetValue<int>(TextureIdProperty, value); }
    }

    /// <summary>缩放模式（"Fill"/"Uniform"/"UniformToFill"/"None"）。</summary>
    public Stretch Stretch {
        get { return this.GetValue<Stretch>(StretchProperty); }
        set { this.SetValue<Stretch>(StretchProperty, value); }
    }

    /// <summary>
    /// 在后端创建动态纹理并绑定到本组件。重新 Attach 前清理旧绑定；登记卸载退订
    /// （元素从树移除时自动 Detach，保证卸载无纹理泄漏）。
    /// </summary>
    /// <returns>纹理 id（1 成功；0 失败）。</returns>
    public int Attach(IRender backend, int width, int height) {
        if (backend == null || width <= 0 || height <= 0) {
            return 0;
        }
        this.Detach(_backend);
        _backend = backend;
        int id = backend.CreateTexture(width, height);
        if (id > 0) {
            this.TextureId = id;
        }
        if (!_detachRegistered) {
            this.RegisterDetach(() => this.Detach(_backend));
            _detachRegistered = true;
        }
        return id;
    }

    /// <summary>把一帧像素上传到绑定纹理（须在后端 BeginFrame 之前调用）。
    /// 上传后触发 <see cref="SurfaceUpdated"/> 并标记帧泵重绘（持续刷新路径）。</summary>
    public void UploadFrame(IRender backend, NativePtr pixels) {
        if (backend == null || this.TextureId <= 0) {
            return;
        }
        backend.UploadTexture(this.TextureId, pixels);
        this.SurfaceUpdated.Set(true);
        this.Invalidate();
    }

    /// <summary>在后端销毁绑定纹理并清零 TextureId；仅接受当前绑定后端。</summary>
    public void Detach(IRender backend) {
        if (backend == null || backend != _backend) {
            return;
        }
        if (this.TextureId > 0) {
            backend.DestroyTexture(this.TextureId);
        }
        this.TextureId = 0;
        _backend = null;
    }

    /// <summary>标记帧泵需重绘（持续刷新路径；每次 UploadFrame 后自动调用，宿主亦可手动）。</summary>
    public void Invalidate() {
        FramePump.Invalidate();
    }
}
