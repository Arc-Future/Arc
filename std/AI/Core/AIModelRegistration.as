// AIModelRegistration — 模型静态声明（RFC 041 §7.2）。
//
// 进程级组合根的注册单元：ModelId 为唯一键（如 "ocr/paddleocr"），重复注册按名
// 覆盖。Factory 为后端注入点（Func<AIModelRegistration, IAIModel>）——注册表
// 不引用任何后端包（依赖方向红线），后端经 OnnxAIModelFactory/IreeAIModelFactory
// 适配（实现 IAIModelFactory）装配。
namespace Arc.AI;

/// <summary>
/// 模型注册声明（RFC 041 §7.2）。<see cref="AIModelRegistry.Register"/> 的入参；
/// 除 ModelId 外的元数据（DisplayName/Kind/Quantization/SizeBytes/Capability/
/// LoadPolicy）供统计 / 预算 / 服务层装配，Factory 是唯一加载入口。
/// </summary>
public class AIModelRegistration {
    /// <summary>唯一键（如 "ocr/paddleocr"、"asr/whisper-small"）。</summary>
    public string ModelId;

    /// <summary>人类可读名称（如 "PaddleOCR"）；空串 = 缺省为 ModelId。</summary>
    public string DisplayName;

    /// <summary>领域类别（Asr/Tts/Ocr/Clip/Face/Embedding/Vision/Generic）。</summary>
    public AIModelKind Kind;

    /// <summary>量化档位（预算/驱动决策用）。</summary>
    public AIModelQuantization Quantization;

    /// <summary>常驻内存估算（预算计账单位，字节）。</summary>
    public long SizeBytes;

    /// <summary>能力标识（默认 "ai.Model"；域子面装配时可为 "ai.Model.Ocr" 等）。</summary>
    public string Capability;

    /// <summary>加载策略（Lazy/Eager/Warm）。</summary>
    public AIModelLoadPolicy LoadPolicy;

    /// <summary>后端注入点：由注册信息创建底层 <see cref="IAIModel"/>（懒加载）。</summary>
    public Func<AIModelRegistration, IAIModel> Factory;

    public AIModelRegistration() {
        this.ModelId = "";
        this.DisplayName = "";
        this.Kind = AIModelKind.Generic;
        this.Quantization = AIModelQuantization.Float32;
        this.SizeBytes = 0;
        this.Capability = "ai.Model";
        this.LoadPolicy = AIModelLoadPolicy.Lazy;
        this.Factory = null;
    }
}
