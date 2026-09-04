// RFC 037 references/texture-surface: DrawTexturePayload 命令载荷。
namespace Arc.UI.Rendering;

/// <summary>DrawTexture 命令载荷（纹理表面——把一张纹理的矩形区域采样绘制到目标矩形）。</summary>
internal struct DrawTexturePayload {
    /// <summary>目标矩形左上角 X（DIP）。</summary>
    public double X;
    /// <summary>目标矩形左上角 Y（DIP）。</summary>
    public double Y;
    /// <summary>目标矩形宽度（DIP）。</summary>
    public double Width;
    /// <summary>目标矩形高度（DIP）。</summary>
    public double Height;
    /// <summary>源矩形 UV 左上（0-1）。</summary>
    public double SrcU0;
    /// <summary>源矩形 UV 左上（0-1）。</summary>
    public double SrcV0;
    /// <summary>源矩形 UV 右下（0-1）。</summary>
    public double SrcU1;
    /// <summary>源矩形 UV 右下（0-1）。</summary>
    public double SrcV1;
    /// <summary>纹理 id（后端侧；0 无效）。</summary>
    public int TextureId;
    /// <summary>透明度（0-1），默认 1.0。</summary>
    public double Alpha;
}
