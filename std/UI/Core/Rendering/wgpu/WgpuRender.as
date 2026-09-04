// RFC 037 §D7.2 + §D10.2: WgpuRender —— wgpu 渲染后端（Arc.UI 唯一后端）。
//
// 本类为 partial，按职责分文件拆分：WgpuRender.Draw.as（绘制图元）/
// Measure.as（测量）/ RenderTree.as（渲染树遍历）/ Wgsl.as（WGSL shader 源码）。
//
// 通过 crates/arc/native/wgpu-native.ani 契约（RFC 016 验证式 FFI）直接调用
// wgpu-native C API，跨平台对接系统 GPU API：
//   - Windows → DirectX 12
//   - macOS   → Metal
//   - Linux   → Vulkan
//   - Web    → WebGPU / WebGL2 兜底
//
// 设计原则（对齐 Arc 高性能章程 + RFC 037 §D7.2）：
//   1. 显式 Command Buffer 编码——所有 Draw* 命令在 BeginFrame/EndFrame
//      之间编码，EndFrame 时一次性 submit，最小化 CPU-GPU 往返。
//   2. 零分配热路径——Instance/Adapter/Device/Queue/Pipeline/UniformBuffer
//      在 Initialize 创建并缓存为字段；帧循环仅 CommandEncoder 创建/finish
//      （wgpu 内部池化，实际无 malloc）。DrawRect 仅写 uniform + emit draw。
//   3. GPU 优先——通过 RenderPipeline + ShaderModule 实现 GPU 硬件加速。
//
// RFC 037 M3.5 矩形绘制架构（uniform buffer pool + 动态偏移 bind group）：
//   - Initialize 创建：
//     · uniform_buffer (64KB) ——单 buffer 池承载所有 DrawRect uniform 数据
//     · bind_group_layout (has_dynamic_offset=true)
//     · bind_group (绑定整个 uniform_buffer)
//     · rect_pipeline (使用新 WGSL shader + bind_group_layout)
//   - BeginFrame：reset uniform_offset = 0
//   - DrawRect：在 offset 写入 48 字节 uniform → set_pipeline → set_bind_group
//     (dynamic_offset=offset) → draw(6, 1, 0, 0) → offset += 256
//   - 渲染路径：CSS 像素坐标 (x, y, w, h) + RGBA → shader 内转 NDC 绘制
//
// 句柄存储设计（RFC 016 §3.3 NativePtr）：
//   所有 wgpu 句柄在 C 侧为 void*，Arc 侧统一存为 NativePtr——
//   LLVM IR 直接对应 `ptr`，调用 .ani 契约函数时零开销透传。
namespace Arc.UI.Rendering.Wgpu;

using Arc.Collections;
using Arc.UI.Components;
using Arc.UI.Internal;
using Arc.UI.Layout;
using Arc.UI.Media;

/// <summary>
/// wgpu-native 渲染后端（Arc.UI 唯一渲染后端）。
///
/// 跨平台 GPU 加速渲染——通过 wgpu-native C API 对接系统 GPU。
/// 同时实现 <see cref="ITextMetrics"/>：布局 Measure 与 DrawText 同源 atlas。
/// </summary>
public partial class WgpuRender : IRender, ITextMetrics {
    // ============================================================
    // 常量
    // ============================================================

    /// <summary>初始化裁剪栈（List 在构造器初始化——字段内联 new 不受支持）。</summary>
    public WgpuRender() {
        _clipX = new List<double>();
        _clipY = new List<double>();
        _clipW = new List<double>();
        _clipH = new List<double>();
        _cmdOffset = new List<int>();
        _cmdPipeline = new List<int>();
        _cmdScissorX = new List<double>();
        _cmdScissorY = new List<double>();
        _cmdScissorW = new List<double>();
        _cmdScissorH = new List<double>();
        _offscreenTargets = new List<NativePtr>();
        _cmdTexture = new List<int>();
        _texTexture = new List<NativePtr>();
        _texView = new List<NativePtr>();
        _texBindGroup = new List<NativePtr>();
        _texW = new List<int>();
        _texH = new List<int>();
        _texInUse = new List<bool>();
    }

    // wgpu 格式/Usage/PresentMode
    private const int WgpuFmtBgra8Unorm = 27;
    private const int WgpuUsageRenderAttachment = 16;
    // WGPUPresentMode：1=Fifo（垂直同步，VSync on）——最兼容，避免 Immediate(3)
    // 在 wgpu-native 下 wgpuSurfaceGetCurrentTexture 阻塞（CPU 忙等 → 窗口卡死）。
    private const int WgpuPresentFifo = 1;
    private const int WgpuPowerHighPerf = 1;

    // uniform buffer pool（RFC 037 M3.5）——8MB = 32768 槽位。
    // 全树渲染（ScrollView 仅 scissor 裁剪、不剔除屏外子树）下单帧命令量与
    // 可见文本字形数成正比（每字形 1 槽），复杂长页面可达数千槽；256KB/1024 槽
    // 会静默丢弃尾部命令（症状：底部内容缺尾、随键入逐字形消失）。上传仅按
    // 帧内实际用量（_uniformOffset 字节），扩容不增加每帧开销。
    private const int UniformBufferSize = 8388608;  // 8MB
    private const int UniformSlotSize = 256;   // 对齐槽位（minUniformBufferOffsetAlignment）
    private const int UniformDataSize = 80;    // 表面填充 uniform（RectUniform = 80 字节）

    // bind group
    private const int BindGroupIndex = 0;
    // WGPUShaderStage_Vertex | WGPUShaderStage_Fragment（rect shader 顶点与片元都读 uniform）
    private const int BindGroupStageVertexFragment = 0x3;

    // 绘制
    private const int RectVertexCount = 6;
    private const int RectBorderThickness = 1.0;
    private const int DrawDefaultInstanceCount = 1;
    private const int DrawDefaultFirstVertex = 0;
    private const int DrawDefaultFirstInstance = 0;

    // wgpu buffer/texture usage 位（vendored webgpu.h 真值）
    private const int WgpuUsageUniform = 0x0040;        // WGPUBufferUsage_Uniform
    private const int WgpuUsageCopyDst = 0x0008;       // WGPUBufferUsage_CopyDst
    // WGPUTextureUsage（vendored 枚举：TextureBinding=0x4, CopyDst=0x2——
    // 与 BufferUsage 位布局不同，需分开定义）
    private const int WgpuTexUsageTextureBinding = 0x0004;
    private const int WgpuTexUsageCopyDst = 0x0002;
    private const int WgpuFormatRgba8Unorm = 22;       // WGPUTextureFormat_RGBA8Unorm (0x16)

    // 布局——内置 8x16 点阵字体（fallback）+ 动态 stb_truetype atlas（主路径）
    private const double LayoutPaddingX = 16.0;
    private const double LayoutPaddingY = 8.0;
    // 8x16 点阵 fallback 常量
    private const double GlyphWidth = 8.0;         // 内置 8x16 点阵字形宽度
    private const double GlyphHeight = 16.0;        // 内置 8x16 点阵字形高度
    private const int GlyphTofuIndex = 95;    // atlas 第 96 槽位（col=15 row=5）：非 ASCII 缺失字形（空心方框 □）
    private const double MinTextPaddingX = 4.0;
    private const double MinTextPaddingY = 2.0;
    // 视口裁剪余量（DIP）：元素 rect 外溢绘制（阴影 offsetY / 焦点环 -2px）不因裁剪丢失
    private const double CullMargin = 8.0;
    // 动态 atlas 基准像素高度（32px = 16px @200% DPI 原生清晰度）
    private const double AtlasBasePx = 32.0;

    // 元素类型名（与 crates/runtime/platform/<os>/window.* 对齐）
    private const string ElStackPanel = "StackPanel";
    private const string ElTextBlock = "TextBlock";
    private const string ElButton = "Button";
    private const string ElToggleButton = "ToggleButton";
    private const string ElRectangle = "Rectangle";
    private const string ElCheckBox = "CheckBox";
    private const string ElTextBox = "TextBox";
    private const string ElImage = "Image";
    private const string ElVideoSurface = "VideoSurface";
    private const string ElScrollView = "ScrollView";
    private const string ElSlider = "Slider";
    private const string ElComboBox = "ComboBox";
    private const string ElGrid = "Grid";
    private const string ElDockPanel = "DockPanel";
    private const string ElWrapPanel = "WrapPanel";
    private const string ElCanvas = "Canvas";
    private const string ElVisualHost = "VisualHost";
    private const string ElListView = "ListView";
    private const string ElDataGrid = "DataGrid";
    private const string ElDataGridRow = "DataGridRow";
    private const string ElWindow = "Window";
    private const string ElElement = "Element";
    private const string ElPopupLayer = "PopupLayer";
    private const string ElPopupBackdrop = "PopupBackdrop";

    // 颜色常量（类型化 Arc.UI.Media.Color；sRGB 分量，渲染端写 uniform 前线性化）
    private static Color ColorTransparent() { return Color.Transparent(); }
    private static Color ColorBorder() { return Color.Parse("#222222"); }
    private static Color ColorTextDefault() { return Color.Parse("#000000"); }

    // 滚动条
    private const double VScrollWidth = 12.0;
    private const double VScrollMinThumb = 20.0;

    // ============================================================
    // 字段
    // ============================================================
    // wgpu 句柄（C 侧 void*，Arc 侧 NativePtr 存储）
    // 所有句柄在 Initialize 创建，Shutdown 释放。
    // 访问规范：private 字段下划线开头，方法体内裸访问（不带 `this.`）。

    /// <summary>wgpu Instance——应用级单例。</summary>
    private NativePtr _instance;

    /// <summary>wgpu Surface——平台窗口绑定的渲染目标。</summary>
    private NativePtr _surface;

    /// <summary>wgpu Adapter——物理 GPU 适配器。</summary>
    private NativePtr _adapter;

    /// <summary>wgpu Device——逻辑设备（命令队列 owner）。</summary>
    private NativePtr _device;

    /// <summary>wgpu Queue——命令提交通道。</summary>
    private NativePtr _queue;

    /// <summary>wgpu ShaderModule——矩形绘制 WGSL shader。</summary>
    private NativePtr _shader;

    /// <summary>wgpu RenderPipeline——矩形绘制管线（带 bind group layout）。</summary>
    private NativePtr _pipeline;

    /// <summary>Surface 当前格式。</summary>
    private int _format;

    /// <summary>当前帧 CommandEncoder。</summary>
    private NativePtr _encoder;

    /// <summary>当前帧 RenderPassEncoder。</summary>
    private NativePtr _pass;

    /// <summary>当前帧 TextureView（render target）。</summary>
    private NativePtr _frameTextureView;

    /// <summary>uniform buffer 池——承载所有 DrawRect uniform 数据。</summary>
    /// <remarks>按 UniformSlotSize 字节槽位对齐。</remarks>
    private NativePtr _uniformBuffer;

    /// <summary>CPU staging 缓冲（RFC 037 P3 阶段1）——整帧 uniform 批写入，
    /// 帧末一次 wgpu_queue_write_buffer 上传，消除逐绘制 GPU 上传。</summary>
    private NativePtr _staging;

    /// <summary>bind group layout（含 1 个 uniform buffer entry，has_dynamic_offset=true）。</summary>
    private NativePtr _bgLayout;

    /// <summary>bind group（绑定整个 uniform buffer）。</summary>
    private NativePtr _bindGroup;

    /// <summary>当前帧 uniform buffer 写入偏移（字节，UniformSlotSize 对齐）。</summary>
    private int _uniformOffset;

    /// <summary>当前帧因槽位耗尽被丢弃的绘制命令数（EndFrame 汇总告警一次；0=无丢弃）。</summary>
    private int _overflowDropped;

    /// <summary>帧命令记录（RFC 037 P3 阶段1）：每次绘制的 uniform 偏移。</summary>
    private List<int> _cmdOffset;

    /// <summary>帧命令记录：每次绘制的 pipeline 类型（0=rect, 1=text），用于去重 set_pipeline。</summary>
    private List<int> _cmdPipeline;

    /// <summary>帧命令记录：每次绘制生效的 scissor 裁剪（DIP 坐标；EndFrame 重放时按需切换）。</summary>
    private List<double> _cmdScissorX;
    private List<double> _cmdScissorY;
    private List<double> _cmdScissorW;
    private List<double> _cmdScissorH;

    /// <summary>当前已 set 的 pipeline 类型（-1=未设置），EndFrame 重放时去重。</summary>
    private int _lastPipeline = -1;

    /// <summary>当前已 set 的 scissor 索引（-1=未设置），EndFrame 重放时去重。</summary>
    private int _lastScissorIdx = -1;

    /// <summary>当前 surface 宽度（像素）——供 DrawRect NDC 转换用。</summary>
    private int _surfaceWidth;

    /// <summary>当前 surface 高度（像素）——供 DrawRect NDC 转换用。</summary>
    private int _surfaceHeight;

    /// <summary>上一次 wgpu_surface_configure 提交的物理像素宽——BeginFrame 据此判断是否需要重新 configure。</summary>
    private int _configuredWidth;

    /// <summary>上一次 wgpu_surface_configure 提交的物理像素高。</summary>
    private int _configuredHeight;

    /// <summary>DPI 缩放系数（DPI/96.0）。布局坐标按 DIP 存储，绘制时乘以该系数换算为物理像素。</summary>
    private double _dpiScale = 1.0;

    /// <summary>布局坐标系尺寸（DIP）——与 ARML Width/Height 一致。</summary>
    private int _dipWidth;

    /// <summary>布局坐标系高度（DIP）。</summary>
    private int _dipHeight;

    // ===== RFC 037 P0 真实裁剪：scissor 裁剪栈 =====

    /// <summary>裁剪栈深度上限（ScrollView/ListView 嵌套深度）。</summary>
    private const int ClipStackMax = 16;

    /// <summary>裁剪栈——每项 4 个 double (x, y, w, h)。栈顶为当前生效裁剪。
    /// 用 List<double>（Arc 数组字段归约为 Named("..._arr") 不支持索引/Add）。</summary>
    private List<double> _clipX;
    private List<double> _clipY;
    private List<double> _clipW;
    private List<double> _clipH;
    private int _clipDepth;

    // ===== RFC 037 M2 文本管线资源（内置点阵字体 + atlas + 纹理采样）=====

    /// <summary>glyph atlas 纹理（128x96 RGBA8，95 可打印 ASCII 字形）。</summary>
    private NativePtr _fontTexture;

    /// <summary>glyph atlas 纹理视图（文本 bind group 绑定）。</summary>
    private NativePtr _fontTextureView;

    /// <summary>nearest 采样器（glyph 硬边）。</summary>
    private NativePtr _sampler;

    /// <summary>文本渲染 WGSL shader。</summary>
    private NativePtr _textShader;

    /// <summary>文本渲染 RenderPipeline（uniform + texture + sampler）。</summary>
    private NativePtr _textPipeline;

    /// <summary>文本渲染 bind group layout（3 entries）。</summary>
    private NativePtr _textBgLayout;

    /// <summary>文本渲染 bind group（uniform buffer + atlas + sampler）。</summary>
    private NativePtr _textBindGroup;

    // ===== RFC 037 M3: 动态 stb_truetype glyph atlas =====

    /// <summary>动态字体 atlas 句柄（NULL 表示使用 8x16 fallback）。</summary>
    private NativePtr _fontAtlas;

    /// <summary>是否处于 fallback 模式（使用 8x16 点阵）。</summary>
    private bool _fontFallback;

    /// <summary>atlas 基准像素高度（AtlasBasePx）。</summary>
    private double _atlasBasePx;

    /// <summary>字体 ascent（基准像素下的物理像素值，正数 = baseline 以上）。</summary>
    private double _fontAscent;

    /// <summary>字体 descent（基准像素下的物理像素值，负数 = baseline 以下）。</summary>
    private double _fontDescent;

    /// <summary>字体 line gap（基准像素下的行间距）。</summary>
    private double _fontLineGap;

    /// <summary>是否已初始化。</summary>
    private bool _initialized;

    /// <summary>已实际 present 的帧数——A-4 帧合并观测点（每 EndFrame 累计 1；一帧多命令合并为一次绘制）。</summary>
    private int _renderCount;

    // ===== RFC 037 §3.5 浮层阴影（Shadow.Surface）：软阴影管线资源 =====

    /// <summary>阴影 WGSL shader。</summary>
    private NativePtr _shadowShader;

    /// <summary>阴影 RenderPipeline（独立 bind group layout，minBindingSize=64）。</summary>
    private NativePtr _shadowPipeline;

    /// <summary>阴影 bind group layout。</summary>
    private NativePtr _shadowBgLayout;

    /// <summary>阴影 bind group（绑定整个 uniform buffer，entry size=64）。</summary>
    private NativePtr _shadowBindGroup;

    // ===== RFC 037 references/texture-surface: 纹理表面（DrawTexture + 动态纹理）=====

    /// <summary>图像采样 WGSL shader（fragment 直出纹理色 * tint）。</summary>
    private NativePtr _imageShader;

    /// <summary>图像渲染 RenderPipeline（复用文本 bind group layout）。</summary>
    private NativePtr _imagePipeline;

    /// <summary>linear 采样器（图像缩放用；文本保持 Nearest）。</summary>
    private NativePtr _imageSampler;

    // 动态纹理注册表（多槽）字段与生命周期见 WgpuRender.Texture.as（RFC 037
    // references/texture-surface）：_texTexture/_texView/_texBindGroup/_texW/
    // _texH/_texInUse。textureId = 槽位+1。

    /// <summary>帧命令记录：image 命令（pipeline 3）的纹理 id（FlushFrameCommands
    /// 重放时按 id 查注册表绑对应 bind group）。</summary>
    private List<int> _cmdTexture;

    /// <summary>
    /// 离屏目标表（List；offscreenId = index+1，销毁槽置 null 不复用——评审目标
    /// 数量极少，避免 Dictionary.Keys 遍历的编译器缺陷（int_arr_get_Count 未定义）。
    /// </summary>
    private List<NativePtr> _offscreenTargets;

    public bool Initialize(long nativeWindowHandle, double width, double height) {
        // 1. 创建 Instance（descriptor 传 null 用默认配置）。
        _instance = wgpu_native.wgpu_create_instance(null);
        if (_instance == null) {
            return false;
        }

        // 2. 从平台原生窗口 handle 创建 Surface。
        //    long 句柄经 C 层 wgpu_create_surface_from_hwnd 转换（规避编译器 CD-29
        //    long→NativePtr cast 缺陷——任意位置的该 cast 都缺 inttoptr，见 plan 登记）。
        _surface = wgpu_native.wgpu_create_surface_from_hwnd(_instance, nativeWindowHandle);
        if (_surface == null) {
            return false;
        }

        // 3-5. Adapter / Device / Queue。
        if (!this.InitializeDevice()) {
            return false;
        }

        // 6. 配置 Surface：首选格式 + RenderAttachment + VSync。
        //    对齐 WGPU.NET `surface.GetPreferredFormat(adapter)`——用 wgpu 报告
        //    的首选格式（跨平台正确性），失败兜底 BGRA8Unorm。
        _format = wgpu_native.wgpu_surface_get_preferred_format(_surface, _adapter);
        if (_format < 0) {
            _format = WgpuFmtBgra8Unorm;
        }
        // DPI 缩放：width/height 为 DIP，surface 物理像素 = DIP * dpi_scale。
        _dpiScale = WindowHost.SystemDpiScale();
        if (_dpiScale < 1.0) { _dpiScale = 1.0; }
        _dipWidth = (int)width;
        _dipHeight = (int)height;
        _surfaceWidth = (int)((double)width * _dpiScale);
        _surfaceHeight = (int)((double)height * _dpiScale);
        int configStatus = wgpu_native.wgpu_surface_configure(
            _surface, _device, _format,
            WgpuUsageRenderAttachment,
            _surfaceWidth, _surfaceHeight,
            WgpuPresentFifo
        );
        if (configStatus != 0) {
            return false;
        }
        _configuredWidth = _surfaceWidth;
        _configuredHeight = _surfaceHeight;

        return this.InitializeRenderResources();
    }

    /// <summary>
    /// 无窗口初始化（离屏渲染专用；headless 可测）。不创建 surface：
    /// _format 固定 RGBA8Unorm（与离屏纹理一致），无 DPI 缩放（物理像素语义）。
    /// </summary>
    public bool InitializeOffscreen() {
        if (!this.InitializeDevice()) {
            return false;
        }
        _format = WgpuFormatRgba8Unorm;
        _dpiScale = 1.0;
        _dipWidth = 0;
        _dipHeight = 0;
        _surfaceWidth = 0;
        _surfaceHeight = 0;
        return this.InitializeRenderResources();
    }

    /// <summary>
    /// 共享设备初始化：Instance → Adapter → Device → Queue。
    /// 窗口模式先建 surface（adapter 请求带兼容 surface）；离屏模式 surface 为 null（可空）。
    /// </summary>
    private bool InitializeDevice() {
        // instance 全类共享单一：窗口模式由 Initialize 先建（surface 与 adapter
        // 同 instance，避免跨 instance 混用 surface → wgpu-core storage panic）；
        // 离屏模式（InitializeOffscreen）无 surface，此处自建。仅空时才创建。
        if (_instance == null) {
            _instance = wgpu_native.wgpu_create_instance(null);
            if (_instance == null) {
                return false;
            }
        }
        _adapter = wgpu_native.wgpu_request_adapter_sync(
            _instance, _surface, WgpuPowerHighPerf
        );
        if (_adapter == null) {
            return false;
        }
        _device = wgpu_native.wgpu_request_device_sync(_adapter, null);
        if (_device == null) {
            return false;
        }
        _queue = wgpu_native.wgpu_device_get_queue(_device);
        if (_queue == null) {
            return false;
        }
        return true;
    }

    /// <summary>
    /// 共享渲染资源初始化：shader / pipeline / bind group / 字体 atlas / 图像采样管线。
    /// 窗口与离屏模式共用（同一渲染面，双宿主）。
    /// </summary>
    private bool InitializeRenderResources() {
        // 7. 创建矩形绘制 WGSL ShaderModule。
        //    源码维护于 std/UI/Rendering/wgpu/rect.wgsl。
        _shader = wgpu_native.wgpu_shader_module_create_wgsl(
            _device, this.RectWgslSource()
        );
        if (_shader == null) {
            return false;
        }

        // 8. 创建 bind group layout（uniform buffer + dynamic offset，
        //    顶点 + 片元阶段都读取 uniform——rect shader 两阶段都要 u）。
        _bgLayout = wgpu_native.wgpu_uniform_bind_group_layout_create(
            _device, BindGroupIndex, BindGroupStageVertexFragment
        );
        if (_bgLayout == null) {
            return false;
        }

        // 9. 创建 uniform buffer（UNIFORM|COPY_DST = 0x0048）。
        _uniformBuffer = wgpu_native.wgpu_buffer_create(
            _device, UniformBufferSize, 0x0048
        );
        if (_uniformBuffer == null) {
            return false;
        }

        // 10. 创建 bind group（绑定整个 uniform buffer）。
        _bindGroup = wgpu_native.wgpu_uniform_bind_group_create(
            _device, _bgLayout, _uniformBuffer
        );
        if (_bindGroup == null) {
            return false;
        }

        // 11. 创建矩形 RenderPipeline（带 bind group layout）。
        _pipeline = wgpu_native.wgpu_render_pipeline_create_rect(
            _device, _shader, _format, _bgLayout
        );
        if (_pipeline == null) {
            return false;
        }

        // 12. RFC 037 M3 文本管线：Nearest 点采样（像素对齐，边缘锐利无模糊）
        _sampler = wgpu_native.wgpu_sampler_create(_device, 0);
        if (_sampler == null) {
            return false;
        }

        // 13. 文本 bind group layout + shader + pipeline（不依赖具体纹理）。
        _textBgLayout = wgpu_native.wgpu_text_bind_group_layout_create(_device);
        if (_textBgLayout == null) {
            return false;
        }
        _textShader = wgpu_native.wgpu_shader_module_create_wgsl(
            _device, this.TextWgslSource()
        );
        if (_textShader == null) {
            return false;
        }
        _textPipeline = wgpu_native.wgpu_text_pipeline_create(
            _device, _textShader, _format, _textBgLayout
        );
        if (_textPipeline == null) {
            return false;
        }

        // 14. 字体 atlas 初始化：先尝试动态 stb_truetype，失败回退 8x16。
        _atlasBasePx = AtlasBasePx;
        _fontAtlas = wgpu_native.wgpu_font_atlas_create(_device, _queue, AtlasBasePx);
        if (_fontAtlas != null && wgpu_native.wgpu_font_atlas_is_fallback(_fontAtlas) == 0) {
            // 动态 atlas 加载成功：使用其 texture view + 真实字体度量。
            _fontFallback = false;
            _fontTexture = null;  // atlas 管理 texture 生命周期
            _fontTextureView = wgpu_native.wgpu_font_atlas_get_texture_view(_fontAtlas);
            _fontAscent = wgpu_native.wgpu_font_atlas_get_ascent(_fontAtlas);
            _fontDescent = wgpu_native.wgpu_font_atlas_get_descent(_fontAtlas);
            _fontLineGap = wgpu_native.wgpu_font_atlas_get_line_gap(_fontAtlas);
        } else {
            // 回退：8x16 点阵（128x96 RGBA atlas）。
            _fontFallback = true;
            if (_fontAtlas != null) {
                wgpu_native.wgpu_font_atlas_destroy(_fontAtlas);
                _fontAtlas = null;
            }
            int atlasWidth = 0;
            int atlasHeight = 0;
            NativePtr atlasPixels = wgpu_native.wgpu_font8x16_build_atlas(out atlasWidth, out atlasHeight);
            if (atlasPixels == null) {
                return false;
            }
            _fontTexture = wgpu_native.wgpu_texture_create_2d(
                _device, atlasWidth, atlasHeight,
                WgpuFormatRgba8Unorm,
                WgpuTexUsageTextureBinding + WgpuTexUsageCopyDst
            );
            if (_fontTexture == null) {
                wgpu_native.wgpu_font_buffer_free(atlasPixels);
                return false;
            }
            wgpu_native.wgpu_texture_write(
                _queue, _fontTexture, atlasWidth, atlasHeight,
                atlasPixels, atlasWidth * atlasHeight * 4
            );
            wgpu_native.wgpu_font_buffer_free(atlasPixels);
            _fontTextureView = wgpu_native.wgpu_texture_create_view(_fontTexture);
            // 8x16 点阵度量（基准 GlyphHeight=16 像素）：ascent≈12, descent≈-4, line_gap=0。
            _fontAscent = 12.0;
            _fontDescent = -4.0;
            _fontLineGap = 0.0;
            _atlasBasePx = GlyphHeight;  // 8x16 基准高度为 16
        }
        if (_fontTextureView == null) {
            return false;
        }

        // 15. 文本 bind group（绑定 uniform buffer + 最终选定的 texture view + sampler）。
        _textBindGroup = wgpu_native.wgpu_text_bind_group_create(
            _device, _textBgLayout, _uniformBuffer, _fontTextureView, _sampler
        );
        if (_textBindGroup == null) {
            return false;
        }

        // 16. RFC 037 §3.5 浮层阴影（Shadow.Surface）：软阴影管线（独立 bind group layout）。
        _shadowShader = wgpu_native.wgpu_shader_module_create_wgsl(
            _device, this.ShadowWgslSource()
        );
        if (_shadowShader == null) {
            return false;
        }
        _shadowBgLayout = wgpu_native.wgpu_shadow_bind_group_layout_create(
            _device, BindGroupIndex, BindGroupStageVertexFragment
        );
        if (_shadowBgLayout == null) {
            return false;
        }
        _shadowBindGroup = wgpu_native.wgpu_shadow_bind_group_create(
            _device, _shadowBgLayout, _uniformBuffer
        );
        if (_shadowBindGroup == null) {
            return false;
        }
        _shadowPipeline = wgpu_native.wgpu_render_pipeline_create_shadow(
            _device, _shadowShader, _format, _shadowBgLayout
        );
        if (_shadowPipeline == null) {
            return false;
        }

        // 17. RFC 037 references/texture-surface：图像采样管线（复用文本 bind group layout
        //     与 pipeline/create 函数，仅 shader 不同——fragment 直出纹理色而非文字 alpha）。
        _imageSampler = wgpu_native.wgpu_sampler_create(_device, 1);  // 1 = Linear（图像缩放）
        if (_imageSampler == null) {
            return false;
        }
        _imageShader = wgpu_native.wgpu_shader_module_create_wgsl(
            _device, this.ImageWgslSource()
        );
        if (_imageShader == null) {
            return false;
        }
        _imagePipeline = wgpu_native.wgpu_text_pipeline_create(
            _device, _imageShader, _format, _textBgLayout
        );
        if (_imagePipeline == null) {
            return false;
        }

        // 渐变走统一表面填充管线（rect pipeline + 80 字节 uniform），无独立渐变轨。

        _uniformOffset = 0;
        _initialized = true;
        if (_surface != null) {
            // 布局同源度量：挂接 ITextMetrics（PrepareForShow 可能早于本初始化，
            // FramePump 随后 RelayoutSynced 用 atlas 重测）。离屏模式无窗口布局，跳过。
            TextMeasuring.Attach(this);
        }
        return true;
    }

    // ===== RFC 037 references/texture-surface：动态纹理注册表（多槽）=====
    // CreateTexture / UploadTexture / DestroyTexture / GetTextureSize /
    // GetTextureBindGroup / DestroyAllTextures 见 WgpuRender.Texture.as。

    public void Resize(double width, double height) {
        if (!_initialized) {
            return;
        }
        _dipWidth = (int)width;
        _dipHeight = (int)height;
        _surfaceWidth = (int)((double)width * _dpiScale);
        _surfaceHeight = (int)((double)height * _dpiScale);
        wgpu_native.wgpu_surface_configure(
            _surface, _device, _format,
            WgpuUsageRenderAttachment,
            _surfaceWidth, _surfaceHeight,
            WgpuPresentFifo
        );
    }

    public void BeginFrame(double width, double height) {
        if (!_initialized) {
            return;
        }
        // 每帧刷新窗口实际 DPI（Per-Monitor：跨屏拖拽后 GetDpiForWindow 变化，
        // 停留旧值会让 surface 尺寸/指针坐标/裁剪全部错配）。
        _dpiScale = WindowHost.SystemDpiScale();
        if (_dpiScale < 1.0) { _dpiScale = 1.0; }
        // 更新 surface 尺寸（窗口 resize 后 BeginFrame 收到新尺寸；DIP → 物理像素）
        _dipWidth = (int)width;
        _dipHeight = (int)height;
        _surfaceWidth = (int)((double)width * _dpiScale);
        _surfaceHeight = (int)((double)height * _dpiScale);
        // 防御：wgpu surface_configure 要求 width/height > 0
        if (_surfaceWidth < 1) { _surfaceWidth = 1; }
        if (_surfaceHeight < 1) { _surfaceHeight = 1; }

        // 尺寸变化时重新 configure surface——wgpu 要求窗口 resize 后显式 configure，
        // 否则 get_current_texture 可能返回 Outdated/Lost 或旧尺寸纹理导致模糊/撕裂。
        if (_surfaceWidth != _configuredWidth || _surfaceHeight != _configuredHeight) {
            int rc = wgpu_native.wgpu_surface_configure(
                _surface, _device, _format,
                WgpuUsageRenderAttachment,
                _surfaceWidth, _surfaceHeight,
                WgpuPresentFifo
            );
            if (rc == 0) {
                _configuredWidth = _surfaceWidth;
                _configuredHeight = _surfaceHeight;
            }
        }

        // 重置 uniform offset——新帧从 0 开始写入
        _uniformOffset = 0;
        _overflowDropped = 0;

        // P3 阶段1：清空帧命令记录，重置 pipeline/scissor 去重状态；惰性创建 CPU staging。
        _cmdOffset.Clear();
        _cmdPipeline.Clear();
        _cmdTexture.Clear();
        _cmdScissorX.Clear();
        _cmdScissorY.Clear();
        _cmdScissorW.Clear();
        _cmdScissorH.Clear();
        _lastPipeline = -1;
        _lastScissorIdx = -1;
        if (_staging == null) {
            _staging = wgpu_native.wgpu_batch_staging_create(UniformBufferSize);
        }

        // 动态 atlas：上传自上帧以来新增的字形（dirty rect）到 GPU。
        // 必须在 acquire frame texture 之前、command encoder 创建之前调用。
        if (!_fontFallback && _fontAtlas != null) {
            wgpu_native.wgpu_font_atlas_flush(_fontAtlas, _queue);
        }

        // 获取当前帧 TextureView——通过 out 参数回写。
        NativePtr view = null;
        int status = wgpu_native.wgpu_surface_get_current_texture(_surface, out view);
        if (status != 0) {
            _frameTextureView = null;
            return;
        }
        _frameTextureView = view;

        // 创建 CommandEncoder + 开始 RenderPass（clear 黑色）。
        _encoder = wgpu_native.wgpu_command_encoder_create(_device);
        _pass = wgpu_native.wgpu_render_pass_begin(
            _encoder,
            _frameTextureView,
            1,            // clear=1
            0.0, 0.0, 0.0, 1.0  // RGBA 黑色
        );
    }

    public void EndFrame() {
        if (!_initialized) {
            return;
        }
        this.FlushFrameCommands(true);
    }

    /// <summary>
    /// 帧命令提交公共路径：上传 staging → 按 pipeline/scissor 去重重放 draw →
    /// pass_end → finish → submit。present=true（窗口帧）：附加 present + 帧计数；
    /// present=false（离屏帧，RenderToOffscreen 用）：不 present、不 release _frameTextureView。
    /// </summary>
    private void FlushFrameCommands(bool present) {
        if (_pass == null) {
            return;
        }
        // 槽位耗尽告警：丢弃的命令意味着画面尾部内容缺失，不可静默。
        if (_overflowDropped > 0) {
            Console.WriteLine("[WGPU-WARN] uniform slots exhausted: dropped " + _overflowDropped
                + " draw cmds this frame (UniformBufferSize=" + UniformBufferSize + ")");
        }
        // P3 阶段1：整帧批提交——一次性上传 staging 到 uniform buffer，
        // 再按 pipeline/scissor 去重重放 draw（消除逐绘制 GPU 上传与 set_pipeline 冗余）。
        int cmdCount = _cmdOffset.Count;
        if (cmdCount > 0 && _staging != null) {
            // 1. 单次 wgpuQueueWriteBuffer 整片上传（N 次逐绘制 GPU 上传 → 1 次）。
            wgpu_native.wgpu_queue_write_buffer(
                _queue, _uniformBuffer, 0, _staging, _uniformOffset);
            // 2. 按 pipeline/scissor 连续段去重 set_pipeline/set_scissor，逐项 set_bind_group(dynamic offset)+draw。
            int lastP = -1;
            int lastS = -1;
            for (int i = 0; i < cmdCount; i++) {
                int p = _cmdPipeline[i];
                // scissor 去重：仅当裁剪区域变化时切换（index 相同即同裁剪）。
                if (i != lastS) {
                    double sx = _cmdScissorX[i];
                    double sy = _cmdScissorY[i];
                    double sw = _cmdScissorW[i];
                    double sh = _cmdScissorH[i];
                    // 与当前生效裁剪比较，不同才切换。
                    bool needSwitch = true;
                    if (lastS >= 0) {
                        double lsx = _cmdScissorX[lastS];
                        double lsy = _cmdScissorY[lastS];
                        double lsw = _cmdScissorW[lastS];
                        double lsh = _cmdScissorH[lastS];
                        if (sx == lsx && sy == lsy && sw == lsw && sh == lsh) {
                            needSwitch = false;
                        }
                    }
                    if (needSwitch) {
                        this.EmitScissor(sx, sy, sw, sh);
                    }
                    lastS = i;
                }
                if (p != lastP) {
                    if (p == 0) {
                        wgpu_native.wgpu_render_pass_set_pipeline(_pass, _pipeline);
                    } else if (p == 2) {
                        wgpu_native.wgpu_render_pass_set_pipeline(_pass, _shadowPipeline);
                    } else if (p == 3) {
                        wgpu_native.wgpu_render_pass_set_pipeline(_pass, _imagePipeline);
                    } else {
                        wgpu_native.wgpu_render_pass_set_pipeline(_pass, _textPipeline);
                    }
                    lastP = p;
                }
                // 选当前命令的 bind group：p==3（image）按 _cmdTexture 查注册表
                //（多槽各绑各纹理视图）；其余用固定共享 bind group。纹理无效
                //（帧内被销毁）防御性跳过绘制，避免 null bind group 崩溃。
                NativePtr bgToUse = null;
                if (p == 0) {
                    bgToUse = _bindGroup;
                } else if (p == 2) {
                    bgToUse = _shadowBindGroup;
                } else if (p == 3) {
                    bgToUse = this.GetTextureBindGroup(_cmdTexture[i]);
                } else {
                    bgToUse = _textBindGroup;
                }
                if (bgToUse != null) {
                    wgpu_native.wgpu_render_pass_set_bind_group(
                        _pass, BindGroupIndex, bgToUse, _cmdOffset[i]);
                    wgpu_native.wgpu_render_pass_draw(
                        _pass, RectVertexCount,
                        DrawDefaultInstanceCount,
                        DrawDefaultFirstVertex,
                        DrawDefaultFirstInstance);
                }
            }
        }
        // 结束 RenderPass + finish encoder + submit；present=true 时附加 present。
        wgpu_native.wgpu_render_pass_end(_pass);
        NativePtr cmd = wgpu_native.wgpu_command_encoder_finish(_encoder);
        wgpu_native.wgpu_queue_submit_one(_queue, cmd);
        if (present) {
            wgpu_native.wgpu_surface_present(_surface);
            wgpu_native.wgpu_release(_frameTextureView);
            _frameTextureView = null;
            // A-4：每 present 累计一帧（一帧内多条绘制命令合并为一次提交/一次绘制）。
            _renderCount = _renderCount + 1;
        }
        wgpu_native.wgpu_release(cmd);
        wgpu_native.wgpu_release(_encoder);
        wgpu_native.wgpu_release(_pass);
        _pass = null;
        _encoder = null;
    }

    // ===== A-1②/A-4 按需渲染观测面（RFC 037 §9.1 A-1② / A-4）=====

    /// <summary>标记本后端需重绘（数据/树结构变更时由渲染层调用；幂等——一帧多变更合并为一次绘制）。</summary>
    public void InvalidateVisual() {
        FramePump.Invalidate();
    }

    /// <summary>已实际渲染帧数（A-4 帧合并观测点——每次 EndFrame/present 累计 1）。</summary>
    public int RenderFrameCount() {
        return _renderCount;
    }

    /// <summary>scissor 下发唯一通道：DIP → 物理像素换算 + 表面边界钳制。
    /// wgpu 校验硬要求 scissor 完全落在 render target 内（x+w≤W 且 y+h≤H），越界即
    /// validation panic 终止进程；空相交/屏外裁剪钳为表面内 0 宽高矩形（绘制全裁、安全下发）。</summary>
    private void EmitScissor(double dipX, double dipY, double dipW, double dipH) {
        int sx = (int)(dipX * _dpiScale);
        int sy = (int)(dipY * _dpiScale);
        int sw = (int)(dipW * _dpiScale);
        int sh = (int)(dipH * _dpiScale);
        if (sx < 0) { sx = 0; }
        if (sy < 0) { sy = 0; }
        if (sx > _surfaceWidth) { sx = _surfaceWidth; }
        if (sy > _surfaceHeight) { sy = _surfaceHeight; }
        if (sw < 0) { sw = 0; }
        if (sh < 0) { sh = 0; }
        if (sx + sw > _surfaceWidth) { sw = _surfaceWidth - sx; }
        if (sy + sh > _surfaceHeight) { sh = _surfaceHeight - sy; }
        wgpu_native.wgpu_render_pass_set_scissor(_pass, sx, sy, sw, sh);
    }

    public void PushClip(double x, double y, double w, double h) {
        if (!_initialized || _pass == null) {
            return;
        }
        // 与当前生效裁剪相交（无裁剪时以 surface 为界，DIP 尺寸——scissor 下发时再乘 _dpiScale）。
        double curX = 0.0;
        double curY = 0.0;
        double curW = (double)_dipWidth;
        double curH = (double)_dipHeight;
        if (_clipDepth > 0) {
            curX = _clipX[_clipDepth - 1];
            curY = _clipY[_clipDepth - 1];
            curW = _clipW[_clipDepth - 1];
            curH = _clipH[_clipDepth - 1];
        }
        double ix1 = (x > curX) ? x : curX;
        double iy1 = (y > curY) ? y : curY;
        double x2 = x + w;
        double y2 = y + h;
        double cx2 = curX + curW;
        double cy2 = curY + curH;
        double ix2 = (x2 < cx2) ? x2 : cx2;
        double iy2 = (y2 < cy2) ? y2 : cy2;
        double iw = ix2 - ix1;
        double ih = iy2 - iy1;
        if (iw < 0.0) { iw = 0.0; }
        if (ih < 0.0) { ih = 0.0; }
        if (_clipDepth < ClipStackMax) {
            _clipX.Add(ix1);
            _clipY.Add(iy1);
            _clipW.Add(iw);
            _clipH.Add(ih);
            _clipDepth++;
        }
        // 空裁剪矩形（完全不相交）→ 0 宽高，后续 draw 全部被裁掉。
        // 裁剪栈存 DIP；下发经 EmitScissor 换算物理像素并钳制到 surface（屏外/空裁剪安全）。
        this.EmitScissor(ix1, iy1, iw, ih);
    }

    public void PopClip() {
        if (!_initialized || _pass == null) {
            return;
        }
        if (_clipDepth > 0) {
            _clipDepth--;
            _clipX.RemoveAt(_clipDepth);
            _clipY.RemoveAt(_clipDepth);
            _clipW.RemoveAt(_clipDepth);
            _clipH.RemoveAt(_clipDepth);
        }
        // 恢复上一裁剪（或全屏）。栈存 DIP → 经 EmitScissor 钳制下发；全屏用 surface（物理，恒在界内）。
        if (_clipDepth > 0) {
            this.EmitScissor(_clipX[_clipDepth - 1], _clipY[_clipDepth - 1],
                _clipW[_clipDepth - 1], _clipH[_clipDepth - 1]);
            return;
        }
        wgpu_native.wgpu_render_pass_set_scissor(
            _pass, 0, 0, _surfaceWidth, _surfaceHeight);
    }

    /// <summary>记录当前生效裁剪到命令列表（供 EndFrame 重放时切换 scissor）。
    /// 无裁剪时记录全 surface DIP 矩形（与 PopClip 恢复一致）。</summary>
    private void RecordCommandScissor() {
        double sx = 0.0;
        double sy = 0.0;
        double sw = (double)_dipWidth;
        double sh = (double)_dipHeight;
        if (_clipDepth > 0) {
            sx = _clipX[_clipDepth - 1];
            sy = _clipY[_clipDepth - 1];
            sw = _clipW[_clipDepth - 1];
            sh = _clipH[_clipDepth - 1];
        }
        _cmdScissorX.Add(sx);
        _cmdScissorY.Add(sy);
        _cmdScissorW.Add(sw);
        _cmdScissorH.Add(sh);
    }

    public void Shutdown() {
        if (!_initialized) {
            return;
        }
        TextMeasuring.Detach(this);
        // 动态纹理注册表（多槽）整体销毁：各槽释放 bind group/view/texture。
        this.DestroyAllTextures();
        // RFC 037 §10 AL-P0：清理离屏目标表（offscreenId = index+1；逐槽销毁并置 null）。
        if (_offscreenTargets != null && _offscreenTargets.Count > 0) {
            for (int i = 0; i < _offscreenTargets.Count; i++) {
                if (_offscreenTargets[i] != null) {
                    wgpu_native.wgpu_offscreen_destroy(_offscreenTargets[i]);
                    _offscreenTargets[i] = null;
                }
            }
        }
        // 释放所有 wgpu 资源（reference counting，Release 减 1）。
        // 顺序：文本管线（text_bind_group → text_pipeline → text_shader →
        //        text_bg_layout → sampler → [atlas|font texture] → font view）→
        //       阴影管线（shadow_pipeline → shadow_shader → shadow_bind_group →
        //       shadow_bg_layout）→ rect pipeline → bind_group → bg_layout →
        //       uniform_buffer → shader → queue → device → adapter → surface → instance。
        wgpu_native.wgpu_release(_textBindGroup);
        wgpu_native.wgpu_release(_textPipeline);
        wgpu_native.wgpu_release(_textShader);
        wgpu_native.wgpu_release(_textBgLayout);
        wgpu_native.wgpu_release(_sampler);
        wgpu_native.wgpu_release(_shadowPipeline);
        wgpu_native.wgpu_release(_shadowShader);
        wgpu_native.wgpu_release(_shadowBindGroup);
        wgpu_native.wgpu_release(_shadowBgLayout);
        if (_fontAtlas != null) {
            // 动态 atlas 的 texture/texture_view 由 atlas 独占管理；
            // Destroy 会释放 texture_view + texture。此处不得再单独
            // release _fontTextureView——它与 a->texture_view 是同一对象，
            // 否则双重释放 → 堆损坏（RFC 037 A-1 修复）。
            wgpu_native.wgpu_font_atlas_destroy(_fontAtlas);
            _fontAtlas = null;
        } else {
            // 8x16 fallback：我们自己创建的 texture view / texture 需手动释放。
            wgpu_native.wgpu_release(_fontTextureView);
            if (_fontTexture != null) {
                wgpu_native.wgpu_release(_fontTexture);
            }
        }
        wgpu_native.wgpu_release(_pipeline);
        wgpu_native.wgpu_release(_bindGroup);
        wgpu_native.wgpu_release(_bgLayout);
        wgpu_native.wgpu_release(_uniformBuffer);
        // P3 阶段1：释放 CPU staging 缓冲。
        if (_staging != null) {
            wgpu_native.wgpu_batch_staging_destroy(_staging);
            _staging = null;
        }
        wgpu_native.wgpu_release(_shader);
        wgpu_native.wgpu_release(_queue);
        wgpu_native.wgpu_release(_device);
        wgpu_native.wgpu_release(_adapter);
        wgpu_native.wgpu_release(_surface);
        wgpu_native.wgpu_release(_instance);
        _initialized = false;
    }
}
