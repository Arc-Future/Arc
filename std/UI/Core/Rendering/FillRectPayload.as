// RFC 037 §6.1 / §6.3: FillRectPayload 命令载荷。
namespace Arc.UI.Rendering;

/// <summary>FillRect 命令载荷。</summary>
internal struct FillRectPayload {
    public double X;
    public double Y;
    public double Width;
    public double Height;
    public string FillColor;
}
