// RFC 041 §7.5：ASR 域请求/响应模型（对齐 OpenAI /v1/audio/transcriptions）。
//
// 字段一一对应 OpenAI 参数/返回值（命名映射见 041 §7.5 编码规范附件）。
// Arc 无可空值类型——Temperature 以 <0 哨兵表示未设置，DurationSeconds 以 <0 表示未上报。
namespace Arc.AI.Models;

using Arc.Collections;

/// <summary>ASR 转写请求（对齐 OpenAI /v1/audio/transcriptions）。</summary>
public class AITranscribeRequest {
    /// <summary>模型标识（本地门面下可省略，由组合根默认 ModelId 填充）。</summary>
    public string Model { get; set; }
    /// <summary>输入音频（FromPcmFloat/FromPcmInt16 值类型工厂）。</summary>
    public AIAudioInput Input { get; set; }
    /// <summary>ISO-639-1 语言（"zh"；null = 自动检测）。</summary>
    public string? Language { get; set; }
    /// <summary>术语/口音引导提示（null = 无）。</summary>
    public string? Prompt { get; set; }
    /// <summary>响应格式（默认 VerboseJson）。</summary>
    public AITranscribeResponseFormat ResponseFormat { get; set; }
    /// <summary>时间戳粒度（Segment/Word；null = 不启用）。</summary>
    public List<AITimestampGranularity>? TimestampGranularities { get; set; }
    /// <summary>采样温度 0..1（&lt;0 = 未设置）。</summary>
    public float Temperature { get; set; }
    /// <summary>流式窗口时长秒（RFC 041 §7.9；&lt;=0 → 30.0，对齐主流 ASR 模型窗口惯例）。</summary>
    public double WindowSeconds { get; set; }

    public AITranscribeRequest() {
        this.Model = "";
        this.Input = null;
        this.Language = null;
        this.Prompt = null;
        this.ResponseFormat = AITranscribeResponseFormat.VerboseJson;
        this.TimestampGranularities = null;
        this.Temperature = (float)-1.0;
        this.WindowSeconds = -1.0;
    }
}

/// <summary>ASR 转写结果（对齐 OpenAI transcription 返回值）。</summary>
public class AITranscribeResult : AIModelResult {
    /// <summary>模型标识（回显）。</summary>
    public string Model { get; set; }
    /// <summary>转写全文。</summary>
    public string Text { get; set; }
    /// <summary>检测语言（ISO-639-1；null = 未检测）。</summary>
    public string? Language { get; set; }
    /// <summary>音频时长（秒；&lt;0 = 未上报）。</summary>
    public double DurationSeconds { get; set; }
    /// <summary>段级结果（verbose_json；null = 非 verbose）。</summary>
    public List<AITranscribeSegment>? Segments { get; set; }
    /// <summary>词级结果（granularity 含 Word；null = 未启用）。</summary>
    public List<AITranscribeWord>? Words { get; set; }
    /// <summary>用量（回显）。</summary>
    public AIUsage Usage { get; set; }

    public AITranscribeResult() {
        this.Model = "";
        this.Text = "";
        this.Language = null;
        this.DurationSeconds = -1.0;
        this.Segments = null;
        this.Words = null;
        this.Usage = new AIUsage();
    }
}

/// <summary>转写段（对齐 OpenAI verbose_json segments）。</summary>
public class AITranscribeSegment {
    /// <summary>段序号。</summary>
    public int Index { get; set; }
    /// <summary>段文本。</summary>
    public string Text { get; set; }
    /// <summary>起始秒（↔ OpenAI start）。</summary>
    public double StartSeconds { get; set; }
    /// <summary>结束秒（↔ OpenAI end）。</summary>
    public double EndSeconds { get; set; }
    /// <summary>段级对数概率。</summary>
    public float AvgLogprob { get; set; }
    /// <summary>无语音概率。</summary>
    public float NoSpeechProb { get; set; }

    public AITranscribeSegment() {
        this.Index = 0;
        this.Text = "";
        this.StartSeconds = 0.0;
        this.EndSeconds = 0.0;
        this.AvgLogprob = (float)0.0;
        this.NoSpeechProb = (float)0.0;
    }
}

/// <summary>转写词（对齐 OpenAI verbose_json words；granularity 含 Word 时）。</summary>
public class AITranscribeWord {
    /// <summary>词文本。</summary>
    public string Text { get; set; }
    /// <summary>起始秒（↔ OpenAI start）。</summary>
    public double StartSeconds { get; set; }
    /// <summary>结束秒（↔ OpenAI end）。</summary>
    public double EndSeconds { get; set; }
    /// <summary>词级对数概率。</summary>
    public float AvgLogprob { get; set; }
    /// <summary>无语音概率。</summary>
    public float NoSpeechProb { get; set; }

    public AITranscribeWord() {
        this.Text = "";
        this.StartSeconds = 0.0;
        this.EndSeconds = 0.0;
        this.AvgLogprob = (float)0.0;
        this.NoSpeechProb = (float)0.0;
    }
}
