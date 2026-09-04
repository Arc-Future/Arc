// RFC 037 references/texture-surface · WgpuRender.Texture —— 动态纹理注册表。
//
// 从「单动态纹理槽」（首版仅 VideoSurface 单一表面）升级为**多槽注册表**：
// Image 多格式（静态位图 / GIF 动画 / SVG）与多个 VideoSurface 可共存，各自
// 独立创建/上传/销毁纹理。textureId = 槽位 index+1（0 恒无效）。
//
// 每槽资源：wgpu 纹理句柄 + 视图 + bind group（复用文本 bind group layout：
// uniform buffer + 纹理视图 + linear 采样器；uniform 经 dynamic offset 共享）。
// 槽位复用：DestroyTexture 置空不紧凑（对齐 _offscreenTargets 先例，避免
// Dictionary.Keys 遍历的编译器缺陷）。
//
// 与帧命令流水线协作：DrawTexture 记录 textureId 到 _cmdTexture，FlushFrameCommands
// 重放时按 textureId 查注册表绑对应 bind group（pipeline 3 = image）。
namespace Arc.UI.Rendering.Wgpu;

using Arc.Collections;

/// <summary>
/// WgpuRender 纹理注册表 partial（本文件承载 CreateTexture/UploadTexture/
/// DestroyTexture + 尺寸/视图查询）。字段与生命周期方法归本职责文件。
/// </summary>
public partial class WgpuRender {
    // ===== RFC 037 references/texture-surface: 动态纹理注册表（多槽）=====

    /// <summary>注册表：纹理句柄（槽位 index+1 = textureId）。</summary>
    private List<NativePtr> _texTexture;

    /// <summary>注册表：纹理视图。</summary>
    private List<NativePtr> _texView;

    /// <summary>注册表：纹理 bind group（uniform + view + linear sampler）。</summary>
    private List<NativePtr> _texBindGroup;

    /// <summary>注册表：纹理宽度（像素）。</summary>
    private List<int> _texW;

    /// <summary>注册表：纹理高度（像素）。</summary>
    private List<int> _texH;

    /// <summary>注册表：槽位占用标记（true=在用；false=已销毁可复用）。</summary>
    private List<bool> _texInUse;

    /// <summary>按 textureId 查槽位（textureId = 槽位+1；无效/未占用返回 -1）。</summary>
    private int TextureSlot(int textureId) {
        int slot = textureId - 1;
        if (slot < 0 || slot >= _texInUse.Count || !_texInUse[slot]) {
            return -1;
        }
        return slot;
    }

    /// <summary>
    /// 创建动态纹理，返回纹理 id（0 失败）。usage 含 TEXTURE_BINDING + COPY_DST。
    /// 纹理内容由 <see cref="UploadTexture"/> 填充，供 DrawTexture 采样。
    /// 槽位不足时复用已销毁槽；无空闲槽追加新槽。
    /// </summary>
    public int CreateTexture(int width, int height) {
        if (!_initialized || width <= 0 || height <= 0) {
            return 0;
        }
        NativePtr tex = wgpu_native.wgpu_texture_create_2d(
            _device, width, height,
            WgpuFormatRgba8Unorm,
            WgpuTexUsageTextureBinding + WgpuTexUsageCopyDst
        );
        if (tex == null) {
            return 0;
        }
        NativePtr view = wgpu_native.wgpu_texture_create_view(tex);
        if (view == null) {
            wgpu_native.wgpu_release(tex);
            return 0;
        }
        NativePtr bindGroup = wgpu_native.wgpu_text_bind_group_create(
            _device, _textBgLayout, _uniformBuffer, view, _imageSampler
        );
        if (bindGroup == null) {
            wgpu_native.wgpu_release(view);
            wgpu_native.wgpu_release(tex);
            return 0;
        }
        // 复用已销毁槽（不紧凑）；无空闲槽则追加。
        int slot = -1;
        int count = _texInUse.Count;
        for (int i = 0; i < count; i++) {
            if (!_texInUse[i]) {
                slot = i;
                break;
            }
        }
        if (slot < 0) {
            slot = count;
            _texTexture.Add(tex);
            _texView.Add(view);
            _texBindGroup.Add(bindGroup);
            _texW.Add(width);
            _texH.Add(height);
            _texInUse.Add(true);
        } else {
            _texTexture[slot] = tex;
            _texView[slot] = view;
            _texBindGroup[slot] = bindGroup;
            _texW[slot] = width;
            _texH[slot] = height;
            _texInUse[slot] = true;
        }
        return slot + 1;
    }

    /// <summary>
    /// 上传整幅像素到纹理（RGBA8，bytesPerRow = width*4）。须在 BeginFrame 之前调用。
    /// data 为像素缓冲 NativePtr（零拷贝透传）。
    /// </summary>
    public void UploadTexture(int textureId, NativePtr data) {
        if (!_initialized || data == null) {
            return;
        }
        int slot = this.TextureSlot(textureId);
        if (slot < 0) {
            return;
        }
        int w = _texW[slot];
        int h = _texH[slot];
        wgpu_native.wgpu_texture_write(
            _queue, _texTexture[slot], w, h, data, w * h * 4
        );
    }

    /// <summary>销毁纹理并释放其视图与 bind group。释放后 textureId 失效。</summary>
    public void DestroyTexture(int textureId) {
        if (!_initialized) {
            return;
        }
        int slot = this.TextureSlot(textureId);
        if (slot < 0) {
            return;
        }
        if (_texBindGroup[slot] != null) {
            wgpu_native.wgpu_release(_texBindGroup[slot]);
            _texBindGroup[slot] = null;
        }
        if (_texView[slot] != null) {
            wgpu_native.wgpu_release(_texView[slot]);
            _texView[slot] = null;
        }
        if (_texTexture[slot] != null) {
            wgpu_native.wgpu_release(_texTexture[slot]);
            _texTexture[slot] = null;
        }
        _texW[slot] = 0;
        _texH[slot] = 0;
        _texInUse[slot] = false;
    }

    /// <summary>按 textureId 查纹理尺寸（像素）。未占用返回 false。</summary>
    internal bool GetTextureSize(int textureId, out int w, out int h) {
        w = 0;
        h = 0;
        int slot = this.TextureSlot(textureId);
        if (slot < 0) {
            return false;
        }
        w = _texW[slot];
        h = _texH[slot];
        return true;
    }

    /// <summary>按 textureId 取 bind group（FlushFrameCommands 重放 image 命令用）。
    /// 未占用返回 null。</summary>
    internal NativePtr GetTextureBindGroup(int textureId) {
        int slot = this.TextureSlot(textureId);
        if (slot < 0) {
            return null;
        }
        return _texBindGroup[slot];
    }

    /// <summary>销毁全部注册表纹理（Shutdown 调用）。</summary>
    private void DestroyAllTextures() {
        if (_texInUse == null) {
            return;
        }
        int count = _texInUse.Count;
        for (int i = 0; i < count; i++) {
            if (_texInUse[i]) {
                this.DestroyTexture(i + 1);
            }
        }
    }
}
