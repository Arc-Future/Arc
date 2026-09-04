// RFC 037 §6.1 / §6.3: DrawLinePayload 命令载荷。
namespace Arc.UI.Rendering;

/// <summary>DrawLine 命令载荷。</summary>
internal struct DrawLinePayload {
    public double X1;
    public double Y1;
    public double X2;
    public double Y2;
    public string Color;
    public double Thickness;
}
