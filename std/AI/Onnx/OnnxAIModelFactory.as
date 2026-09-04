// OnnxAIModelFactory — ONNX 后端适配（RFC 041 §7.2 IAIModelFactory 契约）。
//
// 把 ONNX 后端装配成注册表可注入的工厂：Create 先门闩检查（OnnxModelFactory.
// IsAvailable），不可用抛统一层 AIModelNotAvailableException（注册表原样传播，
// 不降级）；可用则经 OnnxModelFactory.Create 创建 InferenceSession 面。
// UseOnnx() 为组合根装配助手。
namespace Arc.AI.Onnx;

using Arc.AI;

/// <summary>
/// ONNX 后端适配（RFC 041 §7.2）：实现 <see cref="IAIModelFactory"/> 契约，供
/// <see cref="AIModelRegistration.Factory"/> 注入点装配。模型路径构造时固定。
/// </summary>
public class OnnxAIModelFactory : IAIModelFactory {
    private string _modelPath;

    /// <summary>构造 ONNX 后端适配器。</summary>
    /// <param name="modelPath">.onnx 模型文件路径。</param>
    public OnnxAIModelFactory(string modelPath) {
        if (modelPath == null || modelPath == "") {
            throw new ArgumentException("modelPath is required");
        }
        _modelPath = modelPath;
    }

    /// <summary>由注册信息创建 ONNX 推理运行器（门闩检查；不可用抛
    /// <see cref="AIModelNotAvailableException"/>）。</summary>
    public IAIModel Create(AIModelRegistration registration) {
        if (!OnnxModelFactory.IsAvailable) {
            throw new AIModelNotAvailableException(
                "ONNX Runtime native library not available (ARC_ONNX_LIB not set / library missing)");
        }
        return OnnxModelFactory.Create(_modelPath);
    }

    /// <summary>组合根装配助手：构造 ONNX 后端适配器。</summary>
    public static IAIModelFactory UseOnnx(string modelPath) {
        return new OnnxAIModelFactory(modelPath);
    }
}
