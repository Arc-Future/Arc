// RFC 041 §7.5：多模态理解域请求/响应模型（对齐 OpenAI /v1/chat/completions）；后续 P4 · 未接线。
//
// content parts 经 AIUnderstandPart 抽象基类承载（禁 object 袋，§7.5）。Arc 数值无可空
// 哨兵惯例——MaxTokens 以 <=0 表示未设置；ResponseFormat 为枚举无可空哨兵，以 null 表示未启用。
namespace Arc.AI.Models;

using Arc.Collections;

/// <summary>多模态理解请求（对齐 /v1/chat/completions content parts；RFC 041 §7.5；后续 P4 · 未接线）。</summary>
public class AIUnderstandRequest {
    /// <summary>模型标识（本地门面下可省略，由组合根默认 ModelId 填充）。</summary>
    public string Model { get; set; }
    /// <summary>系统提示（↔ messages[role=system].content；null = 无）。</summary>
    public string? SystemPrompt { get; set; }
    /// <summary>多模态输入（content parts：文本/图像）。</summary>
    public List<AIUnderstandPart> Input { get; set; }
    /// <summary>结构化响应格式（null = 纯文本回答）。</summary>
    public AIResponseFormat? ResponseFormat { get; set; }
    /// <summary>最大生成 token 数（&lt;=0 = 未设置）。</summary>
    public int MaxTokens { get; set; }

    public AIUnderstandRequest() {
        this.Model = "";
        this.SystemPrompt = null;
        this.Input = new List<AIUnderstandPart>();
        this.ResponseFormat = null;
        this.MaxTokens = 0;
    }
}

/// <summary>多模态理解结果（对齐 chat completions 返回值；RFC 041 §7.5；后续 P4 · 未接线）。</summary>
public class AIUnderstandResult : AIModelResult {
    /// <summary>模型标识（回显）。</summary>
    public string Model { get; set; }
    /// <summary>回答内容。</summary>
    public string Text { get; set; }
    /// <summary>结束原因（stop / length / content_filter）。</summary>
    public string FinishReason { get; set; }
    /// <summary>用量（回显）。</summary>
    public AIUsage Usage { get; set; }

    public AIUnderstandResult() {
        this.Model = "";
        this.Text = "";
        this.FinishReason = "";
        this.Usage = new AIUsage();
    }
}

/// <summary>多模态输入部件基类（content part；抽象基类禁 object 袋）。</summary>
public abstract class AIUnderstandPart {
}

/// <summary>文本部件（↔ content part type=text）。</summary>
public class AIUnderstandTextPart : AIUnderstandPart {
    /// <summary>文本内容。</summary>
    public string Text { get; set; }

    public AIUnderstandTextPart() {
        this.Text = "";
    }
}

/// <summary>图像部件（↔ content part type=image_url；FromFile/FromPixels 值类型工厂）。</summary>
public class AIUnderstandImagePart : AIUnderstandPart {
    /// <summary>图像输入。</summary>
    public AIImageInput Image { get; set; }

    public AIUnderstandImagePart() {
        this.Image = null;
    }
}
