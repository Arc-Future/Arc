// RFC 041 §7.5：OCR 域请求/响应模型（自定义域 · 对齐 Tesseract 惯例）。
//
// OCR 无 OpenAI 端点 → 对齐领域惯例（Tesseract Confidence 刻度），类型注释显式标注。
// 模型字段名即 OpenAI 参数命名（PascalCase）；本地扩展走显式扩展字段，不冒充 OpenAI 参数。
namespace Arc.AI.Models;

using Arc.Collections;

/// <summary>OCR 请求（RFC 041 §7.5 自定义域）。</summary>
public class AIOcrRequest {
    /// <summary>模型标识（本地门面下可省略，由组合根默认 ModelId 填充）。</summary>
    public string Model { get; set; }
    /// <summary>输入图像（FromPixels 值类型工厂）。</summary>
    public AIImageInput Input { get; set; }
    /// <summary>Tesseract 语言（如 "chi_sim+eng"；null = 引擎默认）。</summary>
    public string? Language { get; set; }

    public AIOcrRequest() {
        this.Model = "";
        this.Input = null;
        this.Language = null;
    }
}

/// <summary>OCR 结果（RFC 041 §7.5）。</summary>
public class AIOcrResult : AIModelResult {
    /// <summary>模型标识（回显）。</summary>
    public string Model { get; set; }
    /// <summary>拼接全文。</summary>
    public string Text { get; set; }
    /// <summary>行级结果（P2 最小：Line 分段为 P4 细化，可空）。</summary>
    public List<AIOcrLine> Lines { get; set; }
    /// <summary>用量（回显）。</summary>
    public AIUsage Usage { get; set; }

    public AIOcrResult() {
        this.Model = "";
        this.Text = "";
        this.Lines = new List<AIOcrLine>();
        this.Usage = new AIUsage();
    }
}

/// <summary>OCR 行结果（RFC 041 §7.5）。</summary>
public class AIOcrLine {
    /// <summary>行文本。</summary>
    public string Text { get; set; }
    /// <summary>行包围盒（x/y/width/height）。</summary>
    public AIRect Box { get; set; }
    /// <summary>4 角点（旋转文本；P2 最小可空）。</summary>
    public List<AIPoint> Quad { get; set; }
    /// <summary>引擎置信度刻度（0..1）。</summary>
    public float Confidence { get; set; }

    public AIOcrLine() {
        this.Text = "";
        this.Box = new AIRect();
        this.Quad = new List<AIPoint>();
        this.Confidence = (float)0.0;
    }
}
