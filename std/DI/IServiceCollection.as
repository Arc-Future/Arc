// IServiceCollection —— 服务注册集合核心接口（RFC 023 M0，对标 .NET IServiceCollection）。
namespace Arc.DI;

/// <summary>
/// 服务注册集合（核心接口最小化——只提供原子方法 Add + Build）。
///
/// AddTransient/AddScoped/AddSingleton/AddKeyed* 等便利方法通过扩展方法提供（P1），
/// 见 std/DI/ServiceCollectionExtensions.as。
/// codegen 只拦截 Add 一个方法达成工厂生成（M1，见 RFC 023 D4.1）。
/// </summary>
public interface IServiceCollection {
    /// <summary>添加一条服务描述符（核心原子方法，codegen 拦截点）。</summary>
    /// <param name="descriptor">服务描述符。</param>
    /// <returns>返回集合自身以支持链式注册。</returns>
    IServiceCollection Add(ServiceDescriptor descriptor);

    /// <summary>构建服务提供者（编译期固化服务描述符表）。</summary>
    /// <returns>已构建的 IServiceProvider 实例。</returns>
    IServiceProvider Build();
}
