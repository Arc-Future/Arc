// RFC 041 §7.5：Arc.AI.Models 枚举（PascalCase ↔ OpenAI 原值，禁自创拼写）。
//
// 每域独立枚举，域间不互用、不抽象公共基类（§7.5「response_format 不跨域统一枚举」）。
// 命名映射见 041 §7.5「编码规范附件 · 命名映射」。
namespace Arc.AI.Models;

/// <summary>ASR 转写响应格式（对齐 OpenAI /v1/audio/transcriptions 的 response_format）。</summary>
public enum AITranscribeResponseFormat {
    /// <summary>"json"</summary>
    Json,
    /// <summary>"text"</summary>
    Text,
    /// <summary>"srt"</summary>
    Srt,
    /// <summary>"verbose_json"</summary>
    VerboseJson,
    /// <summary>"vtt"</summary>
    Vtt
}

/// <summary>时间戳粒度（对齐 OpenAI timestamp_granularities）。</summary>
public enum AITimestampGranularity {
    /// <summary>"segment"</summary>
    Segment,
    /// <summary>"word"</summary>
    Word
}

/// <summary>TTS 合成响应格式（对齐 OpenAI /v1/audio/speech 的 response_format）。</summary>
public enum AITtsResponseFormat {
    /// <summary>"mp3"</summary>
    Mp3,
    /// <summary>"opus"</summary>
    Opus,
    /// <summary>"aac"</summary>
    Aac,
    /// <summary>"flac"</summary>
    Flac,
    /// <summary>"wav"</summary>
    Wav,
    /// <summary>"pcm"</summary>
    Pcm
}

/// <summary>嵌入编码格式（对齐 OpenAI /v1/embeddings 的 encoding_format）。</summary>
public enum AIEncodingFormat {
    /// <summary>"float"（默认）</summary>
    Float,
    /// <summary>"base64"</summary>
    Base64
}

/// <summary>结构化响应格式（对齐 OpenAI response_format，多模态理解/通用）。</summary>
public enum AIResponseFormat {
    /// <summary>"json_object"</summary>
    JsonObject,
    /// <summary>"json_schema"</summary>
    JsonSchema
}
