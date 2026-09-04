// IAIModelFactory — 后端工厂契约（RFC 041 §7.2）。
//
// 注册表不引用任何后端包（依赖方向红线）；加载经
// Func<AIModelRegistration, IAIModel> 工厂注入。本接口是后端包提供的适配契约：
// OnnxAIModelFactory/IreeAIModelFactory 实现 <see cref="Create"/>（把注册信息映射到各自
// 后端工厂），配合 UseOnnx()/UseIree() 装配助手供组合根注入。
namespace Arc.AI;

/// <summary>
/// 后端工厂契约（RFC 041 §7.2）。后端包（Arc.AI.Onnx / Arc.AI.Iree）实现本接口，
/// 注册表侧只消费 <see cref="AIModelRegistration.Factory"/> 注入点，不感知后端。
/// </summary>
public interface IAIModelFactory {
    /// <summary>由注册信息创建底层推理运行器（实现方可先门闩检查可用性）。</summary>
    /// <param name="registration">注册声明（模型元数据，如 ModelId/Capability）。</param>
    /// <returns>推理运行器（<see cref="IAIModel"/> 面，不感知后端差异）。</returns>
    IAIModel Create(AIModelRegistration registration);
}
