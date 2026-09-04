// RFC 041 §7.5：AIModelsOptions — 统一门面全局默认（每域默认模型/超时/重试）。
//
// 单一装配点：每域默认 ModelId 由组合根在构造门面前配好；请求内 Model 字段可省略，
// 显式设置按名覆盖（逐请求覆盖已落地：统计/预算记实际执行模型）。
namespace Arc.AI.Models;

/// <summary>统一门面全局默认（RFC 041 §7.5）。</summary>
public class AIModelsOptions {
    /// <summary>OCR 默认模型（"ocr/paddleocr"）。</summary>
    public string OcrModelId { get; set; }
    /// <summary>ASR 默认模型（"asr/whisper-small"）。</summary>
    public string AsrModelId { get; set; }
    /// <summary>嵌入默认模型。</summary>
    public string EmbedModelId { get; set; }
    /// <summary>CLIP 默认模型（P4）。</summary>
    public string ClipModelId { get; set; }
    /// <summary>人脸默认模型（P4）。</summary>
    public string FaceModelId { get; set; }
    /// <summary>TTS 默认模型（P4）。</summary>
    public string TtsModelId { get; set; }
    /// <summary>多模态理解默认模型（P4）。</summary>
    public string VisionModelId { get; set; }
    /// <summary>单次调用超时（毫秒；0 = 不超时）。</summary>
    public int TimeoutMs { get; set; }
    /// <summary>幂等推理重试次数（默认 0；TTS 等非幂等保持 0）。</summary>
    public int MaxRetries { get; set; }
    /// <summary>重试退避基数（毫秒；指数退避）。</summary>
    public int RetryBackoffMs { get; set; }

    public AIModelsOptions() {
        this.OcrModelId = "ocr/paddleocr";
        this.AsrModelId = "asr/whisper-small";
        this.EmbedModelId = "embed/text-embedding";
        this.ClipModelId = "clip/openclip";
        this.FaceModelId = "face/insightface";
        this.TtsModelId = "tts/vits";
        this.VisionModelId = "vision/qwen-vl";
        this.TimeoutMs = 30000;
        this.MaxRetries = 0;
        this.RetryBackoffMs = 200;
    }

    /// <summary>默认配置。</summary>
    public static AIModelsOptions Default {
        get { return new AIModelsOptions(); }
    }
}
