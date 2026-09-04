// ServiceScope - scope implementation (RFC 023 M1)
namespace Arc.DI;

using Arc.Collections;
using Arc.Reflection;

internal class ServiceScope : IServiceScope, IServiceProvider {
    private ServiceProvider _rootProvider;
    private List<object?> _scoped;
    private List<ServiceDescriptor> _descriptors;
    // 与根容器共享的 Build 后不可变查找结构（扁平 type_id 数组 + 平行索引列表）
    // 与 keyed 二级字典——快照语义，作用域内零构建开销。
    private int[] _typeIds;
    private List<List<int>> _indexLists;
    private Dictionary<int, Dictionary<int, int>> _keyedLookup;
    private bool _disposed;

    public ServiceScope(ServiceProvider provider, List<ServiceDescriptor> descriptors,
        int[] typeIds, List<List<int>> indexLists, Dictionary<int, Dictionary<int, int>> keyedLookup) {
        _rootProvider = provider;
        _descriptors = descriptors;
        _typeIds = typeIds;
        _indexLists = indexLists;
        _keyedLookup = keyedLookup;
        _scoped = new List<object?>();
        for (int i = 0; i < descriptors.Count; i++) {
            _scoped.Add(null);
        }
    }

    public IServiceProvider GetServiceProvider() {
        // 接口返回（IServiceProvider）：codegen 堆盒 + retain obj，借用安全。
        return this;
    }

    /// 内置服务解析——作用域内 IServiceScopeFactory 委托根容器（嵌套 scope 可用），
    /// IServiceProvider 返回作用域自身。对齐 MEDI 语义。
    private object? GetBuiltInService(Type serviceType) {
        if (serviceType.TypeId == typeof(IServiceScopeFactory).TypeId) {
            return _rootProvider;
        }
        if (serviceType.TypeId == typeof(Arc.IServiceProvider).TypeId) {
            return this;
        }
        return null;
    }

    /// <summary>二分定位类型的注册索引列表（与根容器同构：升序唯一 type_id 数组，O(log n)）。</summary>
    private List<int>? FindIndices(int typeId)
    {
        int pos = Array.BinarySearch(_typeIds, typeId);
        if (pos < 0)
        {
            return null;
        }
        return _indexLists[pos];
    }

    public object? GetService(Type serviceType) {
        if (_disposed) {
            throw new ObjectDisposedException("ServiceScope");
        }
        var builtIn = this.GetBuiltInService(serviceType);
        if (builtIn != null) { return builtIn; }

        var indices = this.FindIndices(serviceType.TypeId);
        if (indices == null) { return null; }

        // 最后注册优先（对齐 .NET：同名服务后注册覆盖）。
        var k = indices.Count - 1;
        while (k >= 0) {
            var i = indices[k];
            var desc = _descriptors[i];
            if (desc.Key == null) {
                var fac = desc.Factory;
                if (fac == null) {
                    throw new InvalidOperationException(
                        "Service factory is null for: " + desc.ServiceType.FullName);
                }
                var lt = desc.Lifetime;
                if (lt == ServiceLifetime.Singleton) {
                    return _rootProvider.GetService(serviceType);
                }
                if (lt == ServiceLifetime.Scoped) {
                    if (_scoped[i] == null) {
                        _scoped[i] = fac(this);
                    }
                    // CD-29（同 ServiceProvider）：作用域缓存引用经中间局部赋值转
                    // 新引用——调用方 dec 独立于容器持有，scope.Dispose 的悬垂 dec
                    // 与提前 free（free DUP → UAF）消除。
                    object? instance = _scoped[i];
                    return instance;
                }
                return fac(this);
            }
            k = k - 1;
        }
        return null;
    }

    public object? GetKeyedService(Type serviceType, object? key)
    {
        if (_disposed)
        {
            throw new ObjectDisposedException("ServiceScope");
        }
        // null key 落入默认注册域（与根容器同构，对齐 MEDI null-key 语义）。
        if (key == null)
        {
            return this.GetService(serviceType);
        }

        // keyed O(1) 字典化主路径（与根容器同构）：TryGetValue 区分"未注册"
        // 与"索引 0"（索引器 get 未命中返回 default(int)=0，与合法索引 0 有歧义）。
        var keyMap = _keyedLookup[serviceType.TypeId];
        if (keyMap == null) { return null; }
        string keyValue = (string)key;
        int keyHash = keyValue.GetHashCode();
        int i;
        if (!keyMap.TryGetValue(keyHash, out i))
        {
            return null;
        }
        var desc = _descriptors[i];
        var fac = desc.Factory;
        if (fac == null)
        {
            throw new InvalidOperationException(
                "Service factory is null for: " + desc.ServiceType.FullName);
        }
        var lt = desc.Lifetime;
        if (lt == ServiceLifetime.Singleton)
        {
            return _rootProvider.GetKeyedService(serviceType, key);
        }
        if (lt == ServiceLifetime.Scoped)
        {
            if (_scoped[i] == null)
            {
                _scoped[i] = fac(this);
            }
            // CD-29（同 GetService）：缓存引用经中间局部转新引用。
            object? instance = _scoped[i];
            return instance;
        }
        return fac(this);
    }

    /// <summary>解析某类型的全部 default 注册实例（按注册顺序）；无注册返回空列表。</summary>
    /// <remarks>
    /// 作用域语义与 GetService 一致：Singleton 委托根、Scoped 作用域内缓存、Transient 每次新建。
    /// </remarks>
    public List<object?> GetServices(Type serviceType) {
        if (_disposed) {
            throw new ObjectDisposedException("ServiceScope");
        }
        List<object?> results = new List<object?>();
        var indices = this.FindIndices(serviceType.TypeId);
        if (indices == null) { return results; }
        var k = 0;
        while (k < indices.Count) {
            var i = indices[k];
            var desc = _descriptors[i];
            if (desc.Key == null) {
                var fac = desc.Factory;
                if (fac == null) {
                    throw new InvalidOperationException(
                        "Service factory is null for: " + desc.ServiceType.FullName);
                }
                var lt = desc.Lifetime;
                if (lt == ServiceLifetime.Singleton) {
                    results.Add(_rootProvider.GetService(serviceType));
                } else if (lt == ServiceLifetime.Scoped) {
                    if (_scoped[i] == null) {
                        _scoped[i] = fac(this);
                    }
                    // CD-29（同 GetService）：缓存引用经中间局部转新引用。
                    object? item = _scoped[i];
                    results.Add(item);
                } else {
                    results.Add(fac(this));
                }
            }
            k = k + 1;
        }
        return results;
    }

    public void Dispose() {
        if (_disposed) { return; }
        _disposed = true;
        var i = _scoped.Count - 1;
        while (i >= 0) {
            var instance = _scoped[i];
            if (instance is IDisposable) {
                var d = (IDisposable)instance;
                d.Dispose();
            }
            _scoped[i] = null;
            i = i - 1;
        }
        _scoped = null;
    }
}
