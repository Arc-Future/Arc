// IServiceScopeFactory —— 作用域工厂接口（RFC 023 M0，对标 .NET IServiceScopeFactory）。
namespace Arc.DI;

/// <summary>
/// 作用域工厂——创建新的 IServiceScope（对标 .NET Microsoft.Extensions.DependencyInjection.IServiceScopeFactory）。
///
/// IServiceProvider.CreateScope() 扩展方法内部解析此服务并调用 CreateScope()（P1）。
/// ServiceProvider 实现需将 IServiceScopeFactory 注册为 Singleton 服务（P1）。
/// </summary>
public interface IServiceScopeFactory {
    /// <summary>创建新的作用域。</summary>
    /// <returns>新的 IServiceScope 实例。</returns>
    IServiceScope CreateScope();
}
