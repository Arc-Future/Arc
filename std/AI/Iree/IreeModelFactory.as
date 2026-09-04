// IreeModelFactory — Arc.AI.Iree 公开入口（开箱即用）。
//
// 推理引擎「内部禁止暴露」原则的落地：IreeNative 及后续（M-I1+）的 IreeSession /
// IreeBufferView 等句柄细节均为 internal（内部实现细节），业务侧**只**经本工厂
// 的门闩入口（<see cref="IsAvailable"/> + <see cref="RequireAvailable"/>）做可选
// 功能灰化与受保护启用，不触碰任何原生句柄。
//
// M-I0 交付：门闩 + 降级链（本类 <see cref="IsAvailable"/> == false 时，
// <see cref="RequireAvailable"/> 抛 <see cref="IreeNotAvailableException"/>）。
// M-I1 交付：经 <c>Create</c>（加载 .vmfb → invoke → 读 buffer_view）落成执行后端，
// 以共享抽象 <see cref="Arc.AI.IAIModel"/> 面返回（业务侧不感知后端差异）。
//
// 注：Arc 不支持 static class（无字段承载静态成员），故以普通类承载静态成员
// （对齐 OnnxNative/IreeNative 惯例）。
namespace Arc.AI.Iree;

using Arc;
using Arc.AI;

/// <summary>
/// IREE 推理运行器工厂（公开工厂入口）。经 <see cref="IsAvailable"/> 门闩做可选
/// 功能灰化；<see cref="RequireAvailable"/> 为受保护启用守卫（库不可用抛
/// <see cref="IreeNotAvailableException"/>）。M-I1 起经 <see cref="Create"/> 获得
/// <see cref="Arc.AI.IAIModel"/> 执行推理。RFC 041 §7.2 组合根装配另可经
/// <see cref="IreeAIModelFactory"/> 以 <see cref="Arc.AI.IAIModelFactory"/> 契约注入注册表。
/// </summary>
public class IreeModelFactory {
    /// <summary>IREE Runtime native 库是否可用（`load="auto"` 门闩，用于可选功能灰化）。</summary>
    public static bool IsAvailable {
        get { return IreeNative.IsAvailable; }
    }

    /// <summary>受保护启用守卫：库不可用时抛 <see cref="IreeNotAvailableException"/>
    /// （显式、可捕获——禁静默 stub / 静默 0）。M-I1 起 <c>Create</c> 内部复用本守卫。</summary>
    public static void RequireAvailable() {
        IreeNative.EnsureAvailable();
    }

    /// <summary>加载 .vmfb 模块并创建推理运行器（IREE 函数为位置形参调用）。</summary>
    /// <param name="modulePath">.vmfb 模块文件路径。</param>
    /// <param name="functionName">待执行的导出函数名。</param>
    /// <returns>推理运行器（<see cref="Arc.AI.IAIModel"/> 面，不感知后端差异）。</returns>
    public static IAIModel Create(string modulePath, string functionName) {
        return new IreeSession(modulePath, functionName);
    }
}
