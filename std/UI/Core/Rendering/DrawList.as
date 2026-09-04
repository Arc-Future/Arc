// RFC 037 §6.1: DrawList — 保留模式绘制指令序列。

namespace Arc.UI.Rendering;

using Arc.Collections;

/// <summary>
/// 后端无关 DrawList IR。可序列化、diff、回放；帧末提交 IRender。
/// </summary>
public class DrawList {
    private List<DrawCommand> _commands;

    public DrawList() {
        _commands = new List<DrawCommand>();
    }

    /// <summary>命令数量。</summary>
    public int Count {
        get { return _commands.Count; }
    }

    /// <summary>清空全部命令。</summary>
    public void Clear() {
        _commands.Clear();
    }

    /// <summary>追加单条命令（框架内部；用户经 DrawContext 便捷方法录制）。</summary>
    internal void Add(DrawCommand command) {
        _commands.Add(command);
    }

    /// <summary>追加 DrawText 命令（避免临时 DrawContext 析构误 dec 目标 DrawList）。</summary>
    public void AddDrawText(double x, double y, string text, double fontSize,
                            string foreground, string background) {
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
        _commands.Add(DrawCommand.DrawText(payload));
    }

    /// <summary>追加 DrawTexture 命令（纹理表面——采样纹理矩形到目标矩形）。</summary>
    public void AddDrawTexture(int textureId, double x, double y, double width, double height,
                               double u0, double v0, double u1, double v1, double alpha) {
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
        _commands.Add(DrawCommand.DrawTexture(payload));
    }

    /// <summary>按序读取命令（框架内部：调试 / 执行器遍历）。</summary>
    internal DrawCommand CommandAt(int index) {
        return _commands[index];
    }

    /// <summary>合并另一 DrawList（保留顺序）。</summary>
    public void Append(DrawList other) {
        if (other == null) {
            return;
        }
        for (int i = 0; i < other.Count; i++) {
            _commands.Add(other.CommandAt(i));
        }
    }

    /// <summary>回放至 DrawContext（调试辅助）。</summary>
    public void Execute(DrawContext dc) {
        if (dc == null) {
            return;
        }
        dc.Begin();
        for (int i = 0; i < _commands.Count; i++) {
            DrawCommand cmd = _commands[i];
            switch (cmd)
            {
                case DrawCommand.FillRect(r):
                {
                    dc.FillRect(r.X, r.Y, r.Width, r.Height, r.FillColor);
                    break;
                }
                case DrawCommand.DrawLine(l):
                {
                    dc.DrawLine(l.X1, l.Y1, l.X2, l.Y2, l.Color, l.Thickness);
                    break;
                }
                case DrawCommand.DrawText(t):
                {
                    dc.DrawText(t.X, t.Y, t.Text, t.FontSize, t.Foreground, t.Background);
                    break;
                }
                case DrawCommand.DrawTexture(t):
                {
                    dc.DrawTexture(t.TextureId, t.X, t.Y, t.Width, t.Height,
                                   t.SrcU0, t.SrcV0, t.SrcU1, t.SrcV1, t.Alpha);
                    break;
                }
                default:
                {
                    break;
                }
            }
        }
        dc.End();
    }
}
