// RFC 037 §6.1: DrawContext — 帧内即时绘制录制 API（WPF DrawingContext 精华子集）。

namespace Arc.UI.Rendering;

/// <summary>
/// 帧内即时绘制上下文。Begin/End 之间编码命令至目标 DrawList；
/// 不可跨帧 retain。
/// </summary>
public class DrawContext {
    private DrawList _target;
    private bool _recording;

    public DrawContext(DrawList target) {
        _target = target;
        _recording = false;
    }

    /// <summary>是否处于录制状态。</summary>
    public bool IsRecording {
        get { return _recording; }
    }

    /// <summary>开始录制（不清空目标列表）。</summary>
    public void Begin() {
        _recording = true;
    }

    /// <summary>结束录制并返回目标 DrawList。</summary>
    public DrawList End() {
        _recording = false;
        return _target;
    }

    /// <summary>填充矩形（M-draw1 必达）。</summary>
    public void FillRect(double x, double y, double width, double height, string fillColor) {
        if (!_recording || _target == null) {
            return;
        }
        FillRectPayload payload = new FillRectPayload();
        payload.X = x;
        payload.Y = y;
        payload.Width = width;
        payload.Height = height;
        if (fillColor == null) {
            payload.FillColor = "#FF000000";
        } else {
            payload.FillColor = fillColor;
        }
        _target.Add(DrawCommand.FillRect(payload));
    }

    /// <summary>绘制线段（M-draw1 最小）。</summary>
    public void DrawLine(double x1, double y1, double x2, double y2,
                         string color, double thickness) {
        if (!_recording || _target == null) {
            return;
        }
        DrawLinePayload payload = new DrawLinePayload();
        payload.X1 = x1;
        payload.Y1 = y1;
        payload.X2 = x2;
        payload.Y2 = y2;
        if (color == null) {
            payload.Color = "#FF000000";
        } else {
            payload.Color = color;
        }
        payload.Thickness = thickness;
        _target.Add(DrawCommand.DrawLine(payload));
    }

    /// <summary>
    /// 绘制文本占位（M-draw1：记录命令；glyph/atlas 解释后置 M-draw2）。
    /// </summary>
    public void DrawText(double x, double y, string text, double fontSize,
                         string foreground, string background) {
        if (!_recording || _target == null) {
            return;
        }
        DrawTextPayload payload = new DrawTextPayload();
        payload.X = x;
        payload.Y = y;
        if (text == null) {
            payload.Text = "";
        } else {
            payload.Text = text;
        }
        payload.FontSize = fontSize;
        if (foreground == null) {
            payload.Foreground = "#FF000000";
        } else {
            payload.Foreground = foreground;
        }
        if (background == null) {
            payload.Background = "#FFF4C2";
        } else {
            payload.Background = background;
        }
        _target.Add(DrawCommand.DrawText(payload));
    }

    /// <summary>
    /// 绘制纹理表面（RFC 037 references/texture-surface：采样纹理矩形到目标矩形）。
    /// uv 为源矩形 UV（0-1），默认整幅（0,0,1,1）；alpha 默认 1.0。
    /// </summary>
    public void DrawTexture(int textureId, double x, double y, double width, double height,
                            double u0, double v0, double u1, double v1, double alpha) {
        if (!_recording || _target == null) {
            return;
        }
        DrawTexturePayload payload = new DrawTexturePayload();
        payload.X = x;
        payload.Y = y;
        payload.Width = width;
        payload.Height = height;
        payload.SrcU0 = u0;
        payload.SrcV0 = v0;
        payload.SrcU1 = u1;
        payload.SrcV1 = v1;
        payload.TextureId = textureId;
        payload.Alpha = alpha;
        _target.Add(DrawCommand.DrawTexture(payload));
    }
}
