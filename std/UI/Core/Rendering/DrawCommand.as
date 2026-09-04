// RFC 037 §6.1 / §6.3: DrawCommand 绘制指令 variant。
namespace Arc.UI.Rendering;

/// <summary>后端无关绘制指令（RFC 037 DrawList 条目）。</summary>
internal variant DrawCommand {
    /// <summary>填充矩形。</summary>
    | FillRect of FillRectPayload
    /// <summary>线段（M-draw1 最小解释）。</summary>
    | DrawLine of DrawLinePayload
    /// <summary>文本占位（M-draw1：背景框；glyph 光栅化 M-draw2）。</summary>
    | DrawText of DrawTextPayload
    /// <summary>纹理表面（RFC 037 references/texture-surface：采样纹理到目标矩形）。</summary>
    | DrawTexture of DrawTexturePayload
}
