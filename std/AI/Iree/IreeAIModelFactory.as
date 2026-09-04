// IreeAIModelFactory — IREE 后端适配（RFC 041 §7.2 IAIModelFactory 契约）。
//
// 把 IREE 后端装配成注册表可注入的工厂：Create 先门闩检查（IreeModelFactory.
// IsAvailable），不可用抛统一层 AIModelNotAvailableException（注册表原样传播，
// 不降级）；可用则经 IreeModelFactory.Create 创建 IreeSession 面。
// UseIree() 为组合根装配助手。
namespace Arc.AI.Iree;

using Arc.AI;

/// <summary>
/// IREE 后端适配（RFC 041 §7.2）：实现 <see cref="IAIModelFactory"/> 契约，供
/// <see cref="AIModelRegistration.Factory"/> 注入点装配。模块路径/函数名构造时固定。
/// </summary>
public class IreeAIModelFactory : IAIModelFactory {
    private string _modulePath;
    private string _functionName;

    /// <summary>构造 IREE 后端适配器。</summary>
    /// <param name="modulePath">.vmfb 模块文件路径。</param>
    /// <param name="functionName">待执行的导出函数名。</param>
    public IreeAIModelFactory(string modulePath, string functionName) {
        if (modulePath == null || modulePath == "") {
            throw new ArgumentException("modulePath is required");
        }
        if (functionName == null || functionName == "") {
            throw new ArgumentException("functionName is required");
        }
        _modulePath = modulePath;
        _functionName = functionName;
    }

    /// <summary>由注册信息创建 IREE 推理运行器（门闩检查；不可用抛
    /// <see cref="AIModelNotAvailableException"/>）。</summary>
    public IAIModel Create(AIModelRegistration registration) {
        if (!IreeModelFactory.IsAvailable) {
            throw new AIModelNotAvailableException(
                "IREE Runtime native library not available (ARC_IREE_LIB not set / library missing)");
        }
        return IreeModelFactory.Create(_modulePath, _functionName);
    }

    /// <summary>组合根装配助手：构造 IREE 后端适配器。</summary>
    public static IAIModelFactory UseIree(string modulePath, string functionName) {
        return new IreeAIModelFactory(modulePath, functionName);
    }
}
