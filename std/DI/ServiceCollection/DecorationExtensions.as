// DecorationExtensions —— 装饰器注册扩展（RFC 023 产品化冲刺批次二，对标 .NET Scrutor Decorate）。
// 业界共识：DI 容器不内置装饰器（显式组合优于内置），.NET 生态由 Scrutor 扩展库提供；
// Arc 同样以纯 std 扩展方法承载，编译器零介入。
namespace Arc.DI;

using Arc.Collections;

/// <summary>
/// 服务装饰扩展方法（对标 Scrutor DecorationExtensions）。
/// </summary>
public static class DecorationExtensions {
    /// <summary>
    /// 以 TDecorator 包装 TService 的最后一条 default 注册。
    /// </summary>
    /// <remarks>
    /// TDecorator 构造函数须含一个 TService 形参（接收被包装的内层实例）。
    /// 生命周期沿用被包装注册的 Lifetime；可多次叠加，后 Decorate 者位于洋葱最外层
    /// （解析端最后注册优先）。
    /// </remarks>
    public static IServiceCollection Decorate<TService, TDecorator>(this IServiceCollection services)
        where TDecorator : TService {
        ServiceCollection sc = (ServiceCollection)services;
        // 先快照注册表再定位：NLL 禁止同一承载上"迭代读取 + .Add 追加"混用
        //（迭代器失效误报，同 ServiceCollection.Build 的快照惯例）。注册期一次性
        // O(n) 拷贝，与 Scrutor 的注册表扫描成本一致。
        List<ServiceDescriptor> snapshot = new List<ServiceDescriptor>();
        for (int i = 0; i < sc._descriptors.Count; i++) {
            snapshot.Add(sc._descriptors[i]);
        }
        // 逆序定位最后一条 default 注册——与解析端"最后注册优先"对齐，
        // 保证包装的恰是当前解析会命中的那条注册。
        ServiceDescriptor? last = null;
        for (int i = snapshot.Count - 1; i >= 0; i--) {
            if (snapshot[i].ServiceType.TypeId == typeof(TService).TypeId && snapshot[i].Key == null) {
                last = snapshot[i];
                break;
            }
        }
        if (last == null) {
            throw new InvalidOperationException(
                "Cannot decorate unregistered service: " + typeof(TService).FullName);
        }

        // 捕获旧工厂与生命周期后追加包装注册。装饰链调用捕获的委托引用而非
        // sp.GetService(TService)——解析端最后注册优先会命中装饰注册自身，
        // 自引用即死循环。
        var captured = last.Factory;
        if (captured == null) {
            throw new InvalidOperationException(
                "Service factory is null for: " + typeof(TService).FullName);
        }
        ServiceLifetime lifetime = last.Lifetime;
        return sc.Add(new ServiceDescriptor(
            typeof(TService),
            (sp: IServiceProvider) => (object)new TDecorator((TService)captured(sp)),
            lifetime));
    }
}
