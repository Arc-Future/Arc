// OnnxSetup — ONNX 组合根一行装配糖（RFC 041 §7.2）。
//
// 手工装配需「new AIModelRegistration → 配 Factory = OnnxAIModelFactory.UseOnnx(path)
// → registry.Register」三步样板；本类收敛为一行 AddOnnx(...)，用户「注册表 → 注册
// 模型 → 门面」三行跑通。糖放后端包（依赖方向红线：Core 不引用后端）；加载统一经
// OnnxAIModelFactory 门闩——后端不可用抛 AIModelNotAvailableException，糖层不吞不改。
//
// 注：Arc 不支持 static class（无字段承载静态成员），故以普通类承载静态成员
// （对齐 OnnxModelFactory 惯例）。
namespace Arc.AI.Onnx;

using Arc.AI;

/// <summary>
/// ONNX 组合根装配糖：一行完成「Registration 组装 + OnnxAIModelFactory 工厂注入 +
/// <see cref="AIModelRegistry.Register"/>」。仅收敛样板；SizeBytes（预算计账）/
/// Capability 等进阶元数据走完整手工装配。
/// </summary>
public class OnnxSetup {
    /// <summary>注册 ONNX 模型（Kind=Generic · 懒加载）。</summary>
    /// <param name="registry">目标注册表。</param>
    /// <param name="modelId">模型唯一键（如 "ocr/paddleocr"）。</param>
    /// <param name="modelPath">.onnx 模型文件路径。</param>
    public static void AddOnnx(AIModelRegistry registry, string modelId, string modelPath) {
        OnnxSetup.AddOnnx(registry, modelId, modelPath, AIModelKind.Generic);
    }

    /// <summary>注册 ONNX 模型（指定领域类别 · 懒加载）。</summary>
    /// <param name="registry">目标注册表。</param>
    /// <param name="modelId">模型唯一键。</param>
    /// <param name="modelPath">.onnx 模型文件路径。</param>
    /// <param name="kind">领域类别（Asr/Ocr/Embedding 等）。</param>
    public static void AddOnnx(AIModelRegistry registry, string modelId, string modelPath, AIModelKind kind) {
        OnnxSetup.AddOnnx(registry, modelId, modelPath, kind, AIModelLoadPolicy.Lazy);
    }

    /// <summary>注册 ONNX 模型（指定领域类别与加载策略；Eager/Warm 注册即加载）。</summary>
    /// <param name="registry">目标注册表。</param>
    /// <param name="modelId">模型唯一键。</param>
    /// <param name="modelPath">.onnx 模型文件路径。</param>
    /// <param name="kind">领域类别。</param>
    /// <param name="loadPolicy">加载策略（Lazy/Eager/Warm）。</param>
    public static void AddOnnx(AIModelRegistry registry, string modelId, string modelPath, AIModelKind kind, AIModelLoadPolicy loadPolicy) {
        if (registry == null)
        {
            throw new ArgumentNullException("registry");
        }
        IAIModelFactory backend = OnnxAIModelFactory.UseOnnx(modelPath);
        AIModelRegistration reg = new AIModelRegistration();
        reg.ModelId = modelId;
        reg.Kind = kind;
        reg.LoadPolicy = loadPolicy;
        // 表达式体 lambda 直返接口 + 捕获本地 backend（块体 lambda 隐式
        // 「具体类 → 接口」返回有编译器缺陷，对齐 ai_model_registry_e2e 规避先例）。
        reg.Factory = (r: AIModelRegistration) => backend.Create(r);
        registry.Register(reg);
    }
}
