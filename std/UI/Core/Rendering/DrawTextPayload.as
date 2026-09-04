// RFC 037 §6.1 / §6.3: DrawTextPayload 命令载荷。
namespace Arc.UI.Rendering;

/// <summary>DrawText 占位命令载荷（M-draw1 字形/atlas 后置）。</summary>
internal struct DrawTextPayload {
    public double X;
    public double Y;
    public string Text;
    public double FontSize;
    public string Foreground;
    public string Background;
}
