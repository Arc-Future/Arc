// RFC 041 §7.5：人脸域请求/响应模型（自定义域 · 检测惯例）；后续 P4 · 未接线。
//
// 人脸无 OpenAI 端点 → 对齐领域惯例（Face++/InsightFace 检测框/关键点/置信度）；
// 身份在应用层（§7.5 关键裁决）：结果只含检测/嵌入，身份判定由 VerifyAsync 完成。
namespace Arc.AI.Models;

using Arc.Collections;

/// <summary>人脸检测请求（RFC 041 §7.5 自定义域；后续 P4 · 未接线）。</summary>
public class AIFaceDetectRequest {
    /// <summary>模型标识（本地门面下可省略，由组合根默认 ModelId 填充）。</summary>
    public string Model { get; set; }
    /// <summary>输入图像。</summary>
    public AIImageInput Input { get; set; }

    public AIFaceDetectRequest() {
        this.Model = "";
        this.Input = null;
    }
}

/// <summary>人脸检测结果（RFC 041 §7.5；后续 P4 · 未接线）。</summary>
public class AIFaceDetectResult : AIModelResult {
    /// <summary>模型标识（回显）。</summary>
    public string Model { get; set; }
    /// <summary>检测到的人脸列表。</summary>
    public List<AIFaceDetection> Faces { get; set; }
    /// <summary>用量（回显）。</summary>
    public AIUsage Usage { get; set; }

    public AIFaceDetectResult() {
        this.Model = "";
        this.Faces = new List<AIFaceDetection>();
        this.Usage = new AIUsage();
    }
}

/// <summary>单张人脸检测（自定义域惯例：框/关键点/置信度/可选嵌入）。</summary>
public class AIFaceDetection {
    /// <summary>人脸包围盒。</summary>
    public AIRect Box { get; set; }
    /// <summary>关键点（眼睛/鼻/嘴）。</summary>
    public List<AIPoint> Landmarks { get; set; }
    /// <summary>检测置信度（0..1）。</summary>
    public float Confidence { get; set; }
    /// <summary>识别用嵌入（null = 模型不产出；身份判定在应用层 VerifyAsync）。</summary>
    public AIVector? Embedding { get; set; }

    public AIFaceDetection() {
        this.Box = new AIRect();
        this.Landmarks = new List<AIPoint>();
        this.Confidence = (float)0.0;
        this.Embedding = null;
    }
}
