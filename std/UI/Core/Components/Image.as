// RFC 037 D2.1 + RFC 037 D1: Arc.UI.Components — Image 元素。
//
// Image 是图像显示元素，承载图像源/缩放模式等视觉属性。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Image（WPF 中 Image 直接派生自 FrameworkElement）
//   Arc:  Control → Image（Arc 简化：归到 Control 层以共享 Background 等外观 DP）
//
// **冲突处理（RFC 051 D1 WPF 同构）：**
//   - Width/Height 已由 FrameworkElement 声明——Image 不重复声明，使用继承版本
//   - Image 保留特有 DP：Source/Stretch
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。
//
// **多格式解码 + 动画（RFC 029 M2）**：
//   - Source 变更 → 延迟解码（帧泵首次 tick 时按魔数路由）：
//     GIF → AnimatedGif（多帧 + 每帧延时）；SVG → DecodeSvg 光栅化；其余静态位图
//   - 解码产物经 Arc.Drawing 解码门面（ImageDecoder.Decode/DecodeGif/DecodeSvg），
//     像素句柄（Bitmap.GetPixels / AnimatedGif.Frame）零拷贝上传 GPU 纹理
//   - 纹理由本组件经 IRender 创建/上传/销毁；TextureId 写镜像 handle，渲染端据此采样
//   - GIF 动画：Stopwatch 精确调度帧切换，FramePump 保活（RegisterImage/TickAnimation）

namespace Arc.UI.Components;

using Arc;
using Arc.Diagnostics;
using Arc.Drawing;
using Arc.IO;
using Arc.UI;
using Arc.UI.Internal;
using Arc.UI.Rendering;

/// <summary>图像显示元素。Width/Height 由 FrameworkElement 继承；本类仅声明 Source/Stretch DP。</summary>
public class Image : Control {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public Image() {
        this.Type = typeof(Image);
        // 帧泵保活登记：Source 为 GIF 时持续按帧延时驱动动画（Image 组件自注册）。
        FramePump.RegisterImage(this);
    }

    // ===== 静态依赖属性元数据（RFC 051 D1 WPF 同构）====

    /// <summary>Source 属性元数据——图像源 URL 或资源标识，默认空串。</summary>
    public static DependencyProperty<string> SourceProperty =
        RegisterProperty<string>(nameof(Source), typeof(Image), "");

    /// <summary>Stretch 属性元数据——缩放模式，默认 Stretch.None。</summary>
    public static DependencyProperty<Stretch> StretchProperty =
        RegisterProperty<Stretch>(nameof(Stretch), typeof(Image), Stretch.None);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====
    //
    // **Width/Height 属性继承自 FrameworkElement 基类**——派生类无需重新声明
    // wrapper，直接使用 this.Width / this.Height 即可访问基类 wrapper。

    /// <summary>图像源（文件路径；相对路径相对应用基目录，与字体注册同约定）。</summary>
    public string Source {
        get { return this.GetValue<string>(SourceProperty); }
        set {
            this.SetValue<string>(SourceProperty, value);
            this.OnSourceChanged();
        }
    }

    /// <summary>缩放模式（"Fill"/"Uniform"/"UniformToFill"/"None"）。</summary>
    public Stretch Stretch {
        get { return this.GetValue<Stretch>(StretchProperty); }
        set { this.SetValue<Stretch>(StretchProperty, value); }
    }

    // ===== 解码 + 动画运行时（RFC 029 M2；本组件所有，不上抛渲染层）====

    /// <summary>平台镜像句柄；PlatformTreeSync.BindPlatformMirror 写入（TextureId 同步）。</summary>
    private long _mirrorHandle;

    /// <summary>当前绑定后端（EnsureLoaded/ReleaseDecoded 持有；用于销毁纹理）。</summary>
    private IRender _backend;

    /// <summary>静态/SVG 解码产物（IDisposable；Source 变更时释放）。</summary>
    private Bitmap _bitmap;

    /// <summary>GIF 解码产物（IDisposable；Source 变更时释放）。</summary>
    private AnimatedGif _gif;

    /// <summary>后端纹理 id（0 = 未创建；TextureId 同步到镜像）。</summary>
    private int _textureId;

    /// <summary>GIF 当前帧下标。</summary>
    private int _frameIndex;

    /// <summary>下一帧切换的 Stopwatch 绝对时间戳（0 = 无待切换动画）。</summary>
    private long _nextFrameAt;

    /// <summary>Source 已变更待重载（防重复解码）。</summary>
    private bool _sourceDirty;

    /// <summary>解码 + 纹理已就绪。</summary>
    private bool _loaded;

    /// <summary>Source 变更：释放旧解码/纹理，标脏待重载，请求重绘。</summary>
    void OnSourceChanged() {
        this.ReleaseDecoded();
        _sourceDirty = true;
        FramePump.Invalidate();
    }

    /// <summary>PlatformTreeSync 调用：登记镜像句柄并回写当前 TextureId（0 即占位）。</summary>
    internal void BindPlatformMirror(long handle) {
        _mirrorHandle = handle;
        this.SyncMirrorTexture();
    }

    /// <summary>
    /// 帧泵每次迭代调用：延迟加载（Source 变更或首帧）→ GIF 帧调度推进。
    /// 后端未就绪（null）时跳过，下拍重试；GIF 帧到期则上传并标脏重绘。
    /// </summary>
    internal void TickAnimation(IRender backend) {
        if (backend == null) {
            return;
        }
        _backend = backend;
        if (_sourceDirty || !_loaded) {
            this.EnsureLoaded(backend);
        }
        if (!_loaded || _gif == null) {
            return;
        }
        long now = Stopwatch.GetTimestamp();
        if (_nextFrameAt > 0 && now >= _nextFrameAt) {
            _frameIndex = _frameIndex + 1;
            if (_frameIndex >= _gif.FrameCount) {
                _frameIndex = 0;
            }
            this.UploadFrame(backend);
            _nextFrameAt = now + Image.DelayTicks(_gif.DelayMs(_frameIndex));
            FramePump.Invalidate();
        }
    }

    /// <summary>下一帧切换的绝对时间戳（Stopwatch 域）；无待切换动画返回 0。FramePump 保活用。</summary>
    internal long NextFrameDueAt() {
        if (!_loaded || _gif == null || _nextFrameAt <= 0) {
            return 0;
        }
        return _nextFrameAt;
    }

    /// <summary>
    /// 取用当前纹理（DrawList 预览路径）：已加载且有纹理时返回 true 并输出
    /// 纹理 id 与解码尺寸；未加载/无纹理返回 false（调用方走占位）。
    /// </summary>
    internal bool TryGetTexture(out int textureId, out int width, out int height) {
        textureId = 0;
        width = 0;
        height = 0;
        if (!_loaded || _textureId <= 0) {
            return false;
        }
        textureId = _textureId;
        if (_bitmap != null) {
            width = _bitmap.Width;
            height = _bitmap.Height;
        } else if (_gif != null) {
            width = _gif.Width;
            height = _gif.Height;
        }
        return width > 0 && height > 0;
    }

    /// <summary>按魔数路由解码并创建/上传纹理；解码失败仅 stderr 诊断并保留占位。</summary>
    void EnsureLoaded(IRender backend) {
        _sourceDirty = false;
        string src = this.Source;
        if (src == null || src.Length == 0) {
            return;
        }
        try {
            byte[] data = File.ReadAllBytes(Image.ResolveSourcePath(src));
            if (ImageDecoder.IsGif(data)) {
                _gif = ImageDecoder.DecodeGif(data);
                _frameIndex = 0;
                _textureId = backend.CreateTexture(_gif.Width, _gif.Height);
                if (_textureId <= 0) {
                    _gif.Dispose();
                    _gif = null;
                    return;
                }
                this.UploadFrame(backend);
                _nextFrameAt = Stopwatch.GetTimestamp() + Image.DelayTicks(_gif.DelayMs(0));
            } else {
                if (ImageDecoder.IsSvg(data)) {
                    _bitmap = ImageDecoder.DecodeSvg(data, (float)1.0);
                } else {
                    _bitmap = ImageDecoder.Decode(data);
                }
                _textureId = backend.CreateTexture(_bitmap.Width, _bitmap.Height);
                if (_textureId <= 0) {
                    _bitmap.Dispose();
                    _bitmap = null;
                    return;
                }
                backend.UploadTexture(_textureId, (NativePtr)_bitmap.GetPixels());
            }
            _loaded = true;
        } catch (IOException e) {
            Console.ErrorWriteLine("[Image] decode failed: " + src + " (" + e.Message + ")");
            return;
        }
        this.SyncMirrorTexture();
    }

    /// <summary>把 GIF 当前帧像素上传到绑定纹理（零拷贝句柄透传）。</summary>
    void UploadFrame(IRender backend) {
        if (_gif == null || _textureId <= 0) {
            return;
        }
        backend.UploadTexture(_textureId, (NativePtr)_gif.Frame(_frameIndex));
    }

    /// <summary>TextureId 写镜像 handle（渲染端据此采样；0 = 无纹理占位）。</summary>
    void SyncMirrorTexture() {
        if (_mirrorHandle != 0) {
            WindowHost.ElementSetNumber(_mirrorHandle, "TextureId", (double)_textureId);
        }
    }

    /// <summary>释放解码产物与后端纹理，清零渲染态（Source 变更/重载前调用；幂等）。</summary>
    void ReleaseDecoded() {
        if (_backend != null && _textureId > 0) {
            _backend.DestroyTexture(_textureId);
        }
        _textureId = 0;
        _loaded = false;
        _frameIndex = 0;
        _nextFrameAt = 0;
        if (_bitmap != null) {
            _bitmap.Dispose();
            _bitmap = null;
        }
        if (_gif != null) {
            _gif.Dispose();
            _gif = null;
        }
        this.SyncMirrorTexture();
    }

    /// <summary>毫秒 → Stopwatch 刻度（帧延时换算）。</summary>
    static long DelayTicks(int ms) {
        if (ms < 0) {
            ms = 0;
        }
        return (long)ms * Stopwatch.Frequency / 1000;
    }

    /// <summary>相对 Source 相对应用基目录解析（与字体注册同约定；根路径原样）。</summary>
    static string ResolveSourcePath(string source) {
        if (source == null || source.Length == 0) {
            return source;
        }
        string c0 = source.Substring(0, 1);
        if (c0 == "/" || c0 == "\\") {
            return source;
        }
        if (source.Length >= 2 && source.Substring(1, 1) == ":") {
            return source;
        }
        return Path.Combine(FontManager.GetApplicationBaseDirectory(), source);
    }
}
