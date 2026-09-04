// RFC 041 §7.5：AIRect / AIPoint — 几何值类型（OCR 行包围盒 / 角点）。
//
// 纯数据载体（对齐「强类型、禁 object 袋」）；OCR 行/人脸框/关键点复用。
namespace Arc.AI.Models;

/// <summary>矩形包围盒（x/y 左上角 + 宽高）。</summary>
public class AIRect {
    /// <summary>左上角 X。</summary>
    public float X { get; set; }
    /// <summary>左上角 Y。</summary>
    public float Y { get; set; }
    /// <summary>宽度。</summary>
    public float Width { get; set; }
    /// <summary>高度。</summary>
    public float Height { get; set; }

    public AIRect() {
        this.X = (float)0.0;
        this.Y = (float)0.0;
        this.Width = (float)0.0;
        this.Height = (float)0.0;
    }
}

/// <summary>二维点（归一化或像素坐标，视域而定）。</summary>
public class AIPoint {
    /// <summary>X 坐标。</summary>
    public float X { get; set; }
    /// <summary>Y 坐标。</summary>
    public float Y { get; set; }

    public AIPoint() {
        this.X = (float)0.0;
        this.Y = (float)0.0;
    }
}
