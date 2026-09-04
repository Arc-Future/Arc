// IServiceProvider — 服务解析器核心接口（RFC 023 M0，对齐 .NET System.IServiceProvider）。
namespace Arc;

using Arc.Reflection;
using Arc.Collections;

/// <summary>
/// 服务解析器（对齐 .NET System.IServiceProvider，根命名空间——跨模块通用契约）。
///
/// 核心接口最小化——只提供原子解析方法 GetService + GetKeyedService。
/// GetService&lt;T&gt;/GetRequiredService&lt;T&gt;/GetKeyedService&lt;T&gt;/GetRequiredKeyedService&lt;T&gt;/CreateScope
/// 等便利方法通过扩展方法提供（见 std/DI/ServiceProviderExtensions.as，M1）。
/// QIF Runner、插件宿主、应用启动、ORM DbContext 皆通过此接口解析依赖。
///
/// RFC 018 M2 step 4 / M4: GetService/GetKeyedService 参数从 TypeId 升级为 Type。
/// typeof(T) 直接产生 RuntimeType 实例（Type 子类），赋值给 Type 参数通过多态。
/// ServiceProvider 内部用 Type.TypeId（int）做 O(1) 查找，零反射调用开销。
/// </summary>
public interface IServiceProvider {
    /// <summary>按 Type 解析服务，未注册返回 null（对齐 .NET System.IServiceProvider.GetService）。</summary>
    /// <param name="serviceType">待解析服务的 Type（通常由 typeof(T) 得到）。</param>
    /// <returns>服务实例；未注册返回 null。</returns>
    object? GetService(Type serviceType);

    /// <summary>按 Type + key 解析 keyed 服务，未注册返回 null（.NET 8 keyed services 原子解析）。</summary>
    /// <param name="serviceType">待解析服务的 Type。</param>
    /// <param name="key">keyed 服务的 key（仅字符串，按值相等比较；Arc 无值类型装箱，不支持 int/enum 键）。</param>
    /// <returns>服务实例；未注册返回 null。</returns>
    object? GetKeyedService(Type serviceType, object? key);

    /// <summary>按 Type 解析全部注册实例（含 default 注册），未注册返回空列表（对齐 .NET 的 IEnumerable&lt;T&gt; 解析）。</summary>
    /// <remarks>
    /// 多值解析面：广播（PublishAsync 多 handler）、管道行为链（IPipelineBehavior 有序装配）
    /// 等场景按同一类型取全部实例；元素按注册顺序返回。keyed 注册不入此面。
    /// </remarks>
    /// <param name="serviceType">待解析服务的 Type（通常由 typeof(T) 得到）。</param>
    /// <returns>全部已注册实例列表；无注册时为空列表（非 null）。</returns>
    List<object?> GetServices(Type serviceType);
}
