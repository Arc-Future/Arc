// ServiceCollection - service registration collection (RFC 023 M1)
namespace Arc.DI;

using Arc.Collections;

public class ServiceCollection : IServiceCollection {
    // 注册表内部可枚举（对标 .NET IServiceCollection : IList<ServiceDescriptor>）：
    // 装饰器扩展（DecorationExtensions.Decorate，RFC 023 批次二）同包捕获最后一条
    // 注册的工厂与生命周期。此前因泛型方法单态化沿用消费端包上下文检查成员而
    // 被迫 public（internal 被跨包可见性拒绝）；instantiate_generic_fn 已恢复模板
    // 声明侧包上下文（check_generics），internal 重归包内可见，收敛可见性。
    internal List<ServiceDescriptor> _descriptors;

    public ServiceCollection() {
        _descriptors = new List<ServiceDescriptor>();
    }

    public IServiceCollection Add(ServiceDescriptor descriptor) {
        _descriptors.Add(descriptor);
        return this;
    }

    public IServiceProvider Build() {
        // 构建时快照描述符列表——对齐 .NET（Provider 持有独立副本，构建后对集合的
        // Add 不再影响已构建 Provider，避免共享列表使 _lookup 与 _descriptors 不一致）。
        List<ServiceDescriptor> snapshot = new List<ServiceDescriptor>();
        for (int i = 0; i < _descriptors.Count; i++) {
            snapshot.Add(_descriptors[i]);
        }
        return new ServiceProvider(snapshot);
    }
}
