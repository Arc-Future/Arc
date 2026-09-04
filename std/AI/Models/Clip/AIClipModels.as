// RFC 041 §7.5：CLIP 域请求/响应模型（自定义域 · 嵌入+余弦惯例）；后续 P4 · 未接线。
//
// CLIP 无 OpenAI 端点 → 对齐领域惯例（图文匹配 = 图像/文本嵌入 + 余弦相似度 Score），
// 类型注释显式标注（§7.5「自定义域显式标注」）。
namespace Arc.AI.Models;

using Arc.Collections;

/// <summary>CLIP 图文匹配请求（RFC 041 §7.5 自定义域；后续 P4 · 未接线）。</summary>
public class AIClipMatchRequest {
    /// <summary>模型标识（本地门面下可省略，由组合根默认 ModelId 填充）。</summary>
    public string Model { get; set; }
    /// <summary>查询图像。</summary>
    public AIImageInput Image { get; set; }
    /// <summary>候选文本（零样本分类）。</summary>
    public List<string> Candidates { get; set; }

    public AIClipMatchRequest() {
        this.Model = "";
        this.Image = null;
        this.Candidates = new List<string>();
    }
}

/// <summary>CLIP 图文匹配结果（RFC 041 §7.5；后续 P4 · 未接线）。</summary>
public class AIClipMatchResult : AIModelResult {
    /// <summary>模型标识（回显）。</summary>
    public string Model { get; set; }
    /// <summary>候选相似度列表（顺序对齐请求 Candidates）。</summary>
    public List<AIClipCandidate> Candidates { get; set; }
    /// <summary>用量（回显）。</summary>
    public AIUsage Usage { get; set; }

    public AIClipMatchResult() {
        this.Model = "";
        this.Candidates = new List<AIClipCandidate>();
        this.Usage = new AIUsage();
    }
}

/// <summary>候选文本相似度项（自定义域惯例：余弦相似度/概率）。</summary>
public class AIClipCandidate {
    /// <summary>候选文本。</summary>
    public string Text { get; set; }
    /// <summary>相似度分（余弦相似度/概率）。</summary>
    public float Score { get; set; }

    public AIClipCandidate() {
        this.Text = "";
        this.Score = (float)0.0;
    }
}
