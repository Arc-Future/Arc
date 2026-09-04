// AIModelKind — 小模型领域类别（RFC 041 §7.2）。
//
// 注册表把「被统一管理的一类资产」按领域分类（ASR/OCR/TTS/CLIP/人脸/嵌入/
// 多模态理解），供服务层按域分发与装配；Generic 兜底非域模型。仅分类标签，
// 不引入领域类型进 Arc.AI 核心（语义在 Arc.AI.Models 门面子面表达）。
namespace Arc.AI;

/// <summary>小模型领域类别（RFC 041 §7.2 注册元数据）。</summary>
public enum AIModelKind {
    /// <summary>自动语音识别（语音 → 文本）。</summary>
    Asr,

    /// <summary>语音合成（文本 → 音频）。</summary>
    Tts,

    /// <summary>光学字符识别（图像 → 文本）。</summary>
    Ocr,

    /// <summary>图文匹配（图像/文本嵌入 + 相似度）。</summary>
    Clip,

    /// <summary>人脸检测/识别。身份判定在应用层。</summary>
    Face,

    /// <summary>文本嵌入（向量检索 / 语义索引）。</summary>
    Embedding,

    /// <summary>多模态理解（图片问答等）。</summary>
    Vision,

    /// <summary>未归入特定域的通用模型。</summary>
    Generic,
}
