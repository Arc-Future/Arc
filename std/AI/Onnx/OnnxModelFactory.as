// OnnxModelFactory — Arc.AI.Onnx 公开入口（开箱即用）。
//
// 推理引擎「内部禁止暴露」原则的落地：OnnxTensor / InferenceSession / OnnxNative /
// SessionOptions 的句柄细节均为 internal（内部实现细节），业务侧**只**经本工厂
// 获得 <see cref="Arc.AI.IAIModel"/> 面（宿主张量 <see cref="Arc.AI.Tensor"/>
// 输入/输出），不感知后端差异、不触碰任何原生句柄。
//
// 注：Arc 不支持 static class（无字段承载静态成员），故以普通类承载静态成员
// （对齐 OnnxNative 惯例）。
namespace Arc.AI.Onnx;

using Arc;
using Arc.AI;

/// <summary>
/// ONNX 推理运行器工厂（公开工厂入口）。经 <see cref="Create"/> 获得
/// <see cref="Arc.AI.IAIModel"/> 执行推理；<see cref="IsAvailable"/> 门闩
/// 做可选功能灰化。RFC 041 §7.2 组合根装配另可经 <see cref="OnnxAIModelFactory"/>
/// 以 <see cref="Arc.AI.IAIModelFactory"/> 契约注入注册表。
/// </summary>
public class OnnxModelFactory {
    /// <summary>ONNX Runtime native 库是否可用（`load="auto"` 门闩，用于可选功能灰化）。</summary>
    public static bool IsAvailable {
        get { return OnnxNative.IsAvailable; }
    }

    /// <summary>加载模型并创建推理运行器（默认会话选项：CPU + 默认线程）。</summary>
    /// <param name="modelPath">.onnx 模型文件路径。</param>
    /// <returns>推理运行器（<see cref="Arc.AI.IAIModel"/> 面）。</returns>
    public static IAIModel Create(string modelPath) {
        return new InferenceSession(modelPath);
    }

    /// <summary>加载模型并创建推理运行器（指定会话选项）。</summary>
    /// <param name="modelPath">.onnx 模型文件路径。</param>
    /// <param name="options">会话选项（调用方负责其 Dispose；运行器已拷贝配置）。</param>
    /// <returns>推理运行器（<see cref="Arc.AI.IAIModel"/> 面）。</returns>
    public static IAIModel Create(string modelPath, SessionOptions options) {
        return new InferenceSession(modelPath, options);
    }
}
