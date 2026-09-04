# Arc.UI 纹理表面：VideoSurface 组件与 DrawTexture 绘制原语

> 本文是 实现规划）；实现进度亦不在此维护。
>
> 一主题一文档：本文只讲「把一张动态帧缓冲 / 静态位图作为 UI 表面渲染」。图像解码、二维码、条码属 [029 图像与图形](../../029-imaging-graphics.md)；本文不重复。

## 1. 定位

Arc.UI 当前渲染后端（[WgpuRender.as](../../../../std/UI/Core/Rendering/wgpu/WgpuRender.as)）只支持矩形（uniform-based）、文本（glyph atlas 采样）、阴影三套管线，绘制原语仅 `FillRect / DrawLine / DrawText`（见 [DrawCommand](../../../../std/UI/Core/Rendering/DrawCommand.as)）。本文扩展：

- **`DrawTexture`**——一条后端无关绘制指令 variant，把一张纹理的矩形区域采样绘制到目标矩形。
- **`VideoSurface`**——承载「持续刷新的动态帧缓冲」的 UI 组件（Android 模拟器屏幕、视频播放、地图、摄像头预览等）。

两者为同一内聚主题：`DrawTexture` 是渲染原语，`VideoSurface` 是其组件化消费面。

## 2. 背景与动机

- Android 模拟器 / 视频 / 地图 / 摄像头等场景需要把**外部产生的连续帧缓冲**作为 UI 表面显示。
- 当前渲染后端**无「纹理/位图绘制」原语**：`Image` 元素目前仅画占位矩形（[WgpuRender.RenderTree.as](../../../../std/UI/Core/Rendering/wgpu/WgpuRender.RenderTree.as)），不真正上传/采样位图。
- 底层 wgpu 封装已具备纹理上传与采样管线要素（`wgpu_texture_create_2d` / `wgpu_texture_write` / `wgpu_texture_create_view` / 文本管线即通用纹理采样 quad 管线），本设计以**复用 + 最小扩展**落地，不新开渲染轨。

## 3. 设计决策

### 3.1 `DrawTexture` 绘制原语（命令层）

**绘制指令 variant（语言核心和式类型，见 [004 类型系统](../../004-type-system.md)）**——[DrawCommand](../../../../std/UI/Core/Rendering/DrawCommand.as) 扩展一项：

```as
internal variant DrawCommand {
    | FillRect of FillRectPayload
    | DrawLine of DrawLinePayload
    | DrawText of DrawTextPayload
    | DrawTexture of DrawTexturePayload
}
```

**载荷** `DrawTexturePayload`（新类型，与 `DrawTextPayload` 同构风格）：

| 字段 | 类型 | 语义 |
|------|------|------|
| `X` / `Y` / `Width` / `Height` | double | 目标矩形（DIP，CSS 坐标；渲染端乘 DPI） |
| `SrcU0` / `SrcV0` / `SrcU1` / `SrcV1` | double | 源矩形 UV（0–1），支持局部裁剪/翻转 |
| `TextureId` | int | 帧缓冲纹理句柄（后端侧 id，非 NativePtr 裸指针） |
| `Alpha` | double | 透明度（0–1），默认 1.0 |

**链路**：
- [DrawList](../../../../std/UI/Core/Rendering/DrawList.as) 增加 `AddDrawTexture`，追加 `DrawCommand.DrawTexture`。
- [WgpuRender.Draw.as](../../../../std/UI/Core/Rendering/wgpu/WgpuRender.Draw.as) 增加 `DrawTexture` 方法：按 `DrawText` 同构模式写入 uniform（x/y/w/h + u0/v0/u1/v1 + surface 尺寸）并 `_cmdPipeline.Add(3)`（3 = 图像管线）。
- `EndFrame` 的 pipeline 去重分发（[WgpuRender.as](../../../../std/UI/Core/Rendering/wgpu/WgpuRender.as)）增加 `p == 3` 分支 set 图像管线。
- `IRender` 接口**不新增命令方法**——`DrawTexture` 经既有 `ExecuteDrawList` 主路径消费，保持接口稳定。

**`IRender` 的纹理生命周期契约**（wgpu 唯一后端，单一惯用法）：

```as
public interface IRender {
    // ... 既有 Initialize/Resize/BeginFrame/EndFrame/ExecuteDrawList/...
    /// <summary>创建动态纹理，返回纹理 id（0 失败）。usage 含 TEXTURE_BINDING + COPY_DST。</summary>
    int CreateTexture(int width, int height);
    /// <summary>上传整幅像素到纹理（RGBA8，bytesPerRow = width*4）。须在 BeginFrame 之前调用。</summary>
    void UploadTexture(int textureId, NativePtr data);
    /// <summary>销毁纹理并释放其视图。</summary>
    void DestroyTexture(int textureId);
}
```

> 说明：`UploadTexture` 以 `NativePtr` 零拷贝透传（对齐 `wgpu_texture_write` 既有契约），调用方持有像素缓冲指针；Arc 侧字节数组须先桥到 NativePtr。

### 3.2 图像采样管线（shader / pipeline）

- 复用文本管线的 bind group layout（`uniform@0 动态偏移 + texture@1 + sampler@2`，见 C 侧 `wgpu_text_bind_group_layout_create`）——该布局与文字/图像无关，**直接复用，不新增布局**。
- 新增图像 WGSL shader 变体 `ImageWgslSource()`（顶点与文本同构；片元**直出**纹理色）：

```wgsl
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, smp, in.uv);   // 直出图像 RGBA（区别于文字 tex.a*u.a）
}
```

- 图像管线经既有 `wgpu_text_pipeline_create` 创建路径以新 shader 实例化；bind group 经既有 `wgpu_text_bind_group_create` 绑定目标纹理视图 + 采样器。

### 3.3 `VideoSurface` 组件（组件化消费面）

命名空间 `Arc.UI.Components`，派生自 `Control`（继承 `Width/Height` 等 DP，对齐 `Image` 归 `Control` 层的既有决定）。

```as
public class VideoSurface : Control {
    // 静态依赖属性
    public static DependencyProperty<int> TextureIdProperty;   // 关联纹理 id
    public static DependencyProperty<Stretch> StretchProperty; // 缩放模式（None/Uniform/...）
    // 生命周期
    public void Attach(int textureId);   // 绑定后端纹理 id
    public void Detach();
    public void Invalidate();            // 标记本表面需重绘（每帧/脏区）
    // 信号
    public Signal<bool> SurfaceUpdated;
}
```

**职责**：
- **显示**：作为 `DrawTexture` 的目标矩形，由渲染端在遍历到本元素时（`RenderTree` / DrawList）录制一条 `DrawTexture` 命令。
- **持续刷新**：`VideoSurface` 走「持续渲染」路径（复用 `MotionEngine.Active()` 强制每帧渲染的先例），保证视频帧不因脏区按需渲染而被跳过。
- **生命周期**：`Attach` 时确保后端已 `CreateTexture`；`Detach`/卸载时 `DestroyTexture`，杜绝泄漏。

**边界（本组件不承担）**：
- **输入注入**：坐标级触摸/键盘转发给 guest（Android 模拟器交互）为后续能力，不在本面——本面仅「显示」。
- **音频**：Arc.UI 无音频能力，不在此篇。
- **模拟器内核集成**：QEMU/自定义 CPU 仿真属原生组件，经 FFI/子进程接入，非 Arc.UI 职责。

### 3.4 动态帧缓冲上传（生命周期管理 / 时机 / 格式）

- **生命周期**：`VideoSurface.Attach` 触发 `CreateTexture`（尺寸固定或按需重建）；每帧 `UploadTexture` 更新内容；`Detach`/卸载 `DestroyTexture`。
- **时机**：上传必须在 **command encoder 创建之前**（`wgpuQueueWriteTexture` 不能插入 render pass 中间），对齐 [WgpuRender.as](../../../../std/UI/Core/Rendering/wgpu/WgpuRender.as) 中 `wgpu_font_atlas_flush` 的时机。
- **格式对齐**：surface 首选 BGRA8Unorm，Android 帧缓冲多为 RGBA——须在源头以目标格式上传或 C 侧转换，避免逐像素 CPU 转换。
- **性能**：全帧 1080×2400 RGBA ≈ 10MB/帧。采用 staging buffer 池避免每帧 malloc（复用批量上传模式）；脏区更新优先。

### 3.5 采样器过滤（C 层唯一必需改动）

现有 `wgpu_sampler_create` 硬编码 `Nearest`（文字硬边）。图像缩放需 `Linear`。将过滤模式**参数化**（新增采样器变体或加参），保持 `Nearest` 为文字默认。

## 4. 单一惯用法与架构红线

- **不新开渲染轨**：纹理表面复用既有 wgpu 管线 + 文本管线结构，不引入第二渲染后端。
- **不重复文档**：图像解码/二维码归 029；本文只讲「帧缓冲/纹理作为 UI 表面渲染」。
- **编译器核心不承载领域逻辑**：本能力属标准库 UI 层（`std/UI/Core/`），仅消费既有语言能力与 `.ani` FFI 机制。

## 5. 落地路径（供 实现规划 排期）

1. C 层：采样器过滤参数化（唯一必需的 C 改动；其余 `create_2d/create_view/texture_write/text_bind_group_*` 全复用）。
2. shader：新增 `ImageWgslSource()` + 图像 pipeline。
3. 命令层：`DrawCommand.DrawTexture` + `DrawTexturePayload` + `DrawList.AddDrawTexture` + `WgpuRender.DrawTexture` + `EndFrame` pipeline 分支。
4. 组件层：`VideoSurface`（生命周期 + 持续刷新 + `DrawTexture` 录制）。
5. `IRender` 纹理契约：`CreateTexture / UploadTexture / DestroyTexture`。

## 6. 边界（不在此篇）

- 输入注入 / 交互路由（后续能力）。
- 音频输出。
- Android 模拟器内核本身（QEMU 等原生组件）。
- 图像解码格式（见 029）。

---

[返回 037 主题入口(../../037-ui.md) · [返回 RFC 索引](../../index.md)
