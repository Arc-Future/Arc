// RFC 041 §7.5：TTS 域请求/响应模型（对齐 OpenAI /v1/audio/speech）；后续 P4 · 未接线。
//
// 字段一一对应 OpenAI 参数/返回值（命名映射见 041 §7.5 编码规范附件）。
// TTS 无 token 计数——Usage 各计数按 AIUsage 惯例以 -1 哨兵表示未上报。
namespace Arc.AI.Models;

using Arc.Collections;

/// <summary>TTS 合成请求（对齐 OpenAI /v1/audio/speech；RFC 041 §7.5；后续 P4 · 未接线）。</summary>
public class AITtsRequest {
    /// <summary>模型标识（本地门面下可省略，由组合根默认 ModelId 填充）。</summary>
    public string Model { get; set; }
    /// <summary>合成文本。</summary>
    public string Input { get; set; }
    /// <summary>声线（模型相关，如 "alloy"）。</summary>
    public string Voice { get; set; }
    /// <summary>音频格式（默认 Mp3）。</summary>
    public AITtsResponseFormat ResponseFormat { get; set; }
    /// <summary>语速 0.25..4.0（默认 1.0）。</summary>
    public float Speed { get; set; }
    /// <summary>语音风格指令（null = 无）。</summary>
    public string? Instructions { get; set; }
    /// <summary>流式切句句长上限兜底（RFC 041 §7.9；&lt;=0 → 120，防无标点长文一块到底）。</summary>
    public int MaxChunkChars { get; set; }

    public AITtsRequest() {
        this.Model = "";
        this.Input = "";
        this.Voice = "";
        this.ResponseFormat = AITtsResponseFormat.Mp3;
        this.Speed = (float)1.0;
        this.Instructions = null;
        this.MaxChunkChars = 0;
    }
}

/// <summary>TTS 合成结果（对齐 OpenAI audio/speech 返回值；RFC 041 §7.5；后续 P4 · 未接线）。</summary>
public class AITtsResult : AIModelResult {
    /// <summary>模型标识（回显）。</summary>
    public string Model { get; set; }
    /// <summary>合成音频（PCM float + SampleRate；解码后的语义值类型）。</summary>
    public AIAudioInput Audio { get; set; }
    /// <summary>音频格式（回显）。</summary>
    public AITtsResponseFormat ResponseFormat { get; set; }
    /// <summary>用量（回显；TTS 无 token 计数 → 各计数 -1 未上报）。</summary>
    public AIUsage Usage { get; set; }

    public AITtsResult() {
        this.Model = "";
        this.Audio = new AIAudioInput();
        this.ResponseFormat = AITtsResponseFormat.Mp3;
        this.Usage = new AIUsage();
    }
}

/// <summary>TTS 流式音频块（RFC 041 §7.9）：SynthesizeStreamAsync 增量投递单元。</summary>
public class AITtsChunk : AIModelResult {
    /// <summary>本块 PCM float 采样（模型输出 Float32 [M]；容器编码属应用层）。</summary>
    public List<float> Samples { get; set; }
    /// <summary>块序号（0 起递增）。</summary>
    public int Index { get; set; }
    /// <summary>是否末块（完成前置标记）。</summary>
    public bool IsFinal { get; set; }
    /// <summary>本块对应切句文本（供字幕对齐与调试）。</summary>
    public string Text { get; set; }

    public AITtsChunk() {
        this.Samples = new List<float>();
        this.Index = 0;
        this.IsFinal = false;
        this.Text = "";
    }
}
