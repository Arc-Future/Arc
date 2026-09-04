// ServiceProvider — 根服务提供者（RFC 023 M1）
//
// v1.1 性能修订 (2026-08-17, RFC 023 冲刺批次一/二):
//   - GetService 主路径扁平化: 字典哈希查找 → 排序 type_id 数组 + 二分定位
//     （O(log n)、无哈希、无装箱），Build 后两结构不可变（快照语义）
//   - GetKeyedService 二级字典化: (type_id, key 哈希) → descriptor 索引 O(1)，
//     消除索引列表上的逆序线性扫描
//
// v1.0 生产级修订 (2026-07-24):
//   - 并发安全: Singleton 在 Build() 时预构造，运行时只读列表——零竞态、零锁
//   - 生命周期安全: Build 时循环依赖检测 + Factory null 防御 + Dispose 异常安全
//   - 性能: GetService 仅 List 索引读取 O(1)
//   - 严谨性: ObjectDisposedException 替代静默返回 null
//
// 设计决策——Singleton Build 时预构造:
//   1. 消除运行时竞态——Build 单线程内构造完成，运行时只读、无 check-then-act，
//      无需锁原语（优于 .NET 的 Lazy<T> 运行时加锁，零锁开销）
//   2. 运行时零开销——GetService 仅 _singletons[i] 列表读取
//   3. 启动时即暴露依赖错误——未注册/循环依赖在 Build 时报错而非运行时
//   4. 对齐 MEDI ValidateScopes 语义——.NET 8 默认不预构造，Arc 做得更严
//
// 开放问题 Q2 决议: Singleton 线程安全通过 Build 时预构造解决——
// 运行时 _singletons 为不可变只读列表，天然线程安全。
namespace Arc.DI;

using Arc.Collections;
using Arc.Reflection;

internal class ServiceProvider : IServiceProvider, IServiceScopeFactory, IDisposable {
    private List<ServiceDescriptor> _descriptors;
    private List<object?> _singletons;
    private List<int> _constructing;
    // 扁平查找结构（批次二）: _typeIds 升序唯一，_indexLists 与其平行——
    // 二分定位后按注册顺序枚举，承载多注册的 List<int> 语义不变。
    private int[] _typeIds;
    private List<List<int>> _indexLists;
    // keyed 二级字典（批次一）: type_id → (key 哈希 → 最后注册的 descriptor 索引)。
    // 内键用 key 字符串的实例 GetHashCode（DJB2 内容哈希，rt_hash_str 无随机种子）——
    // 确定性是硬要求，禁止引入非确定性哈希（Arc 编译期确定性哲学）。
    private Dictionary<int, Dictionary<int, int>> _keyedLookup;
    private bool _disposed;

    public ServiceProvider(List<ServiceDescriptor> descriptors)
    {
        _descriptors = descriptors;
        _singletons = new List<object?>();
        for (int i = 0; i < descriptors.Count; i++)
        {
            _singletons.Add(null);
        }

        // 构建期临时分组: TypeId → 描述符索引列表（单趟：去重开组 + 按注册顺序回填）。
        var byType = new Dictionary<int, List<int>>();
        for (int j = 0; j < descriptors.Count; j++)
        {
            int typeVal = descriptors[j].ServiceType.TypeId;
            if (!byType.ContainsKey(typeVal))
            {
                byType.Add(typeVal, new List<int>());
            }
            byType[typeVal].Add(j);
        }

        // 扁平化（批次二）: 借临时字典去重分组后，冻结为"排序 type_id 数组 + 平行
        // 索引列表"。主路径二分定位无哈希查找、无装箱；临时字典随之丢弃，
        // 运行期只持有两个不可变结构（快照语义），无新旧双轨查找路径。
        List<int> sortedIds = new List<int>();
        for (int j = 0; j < descriptors.Count; j++)
        {
            sortedIds.Add(descriptors[j].ServiceType.TypeId);
        }
        int[] localTypeIds = sortedIds.ToArray();
        // 空数组边界：runtime Array.Sort 对空缓冲（null 哨兵）直接崩溃，空时跳过
        // 归一化排序（空排序语义上即无操作）；无注册的 Provider 亦不触发后续查找。
        if (sortedIds.Count > 0)
        {
            Array.Sort(localTypeIds);
        }
        List<int> uniqueIds = new List<int>();
        _indexLists = new List<List<int>>();
        for (int s = 0; s < localTypeIds.Length; s++)
        {
            if (s == 0 || localTypeIds[s] != localTypeIds[s - 1])
            {
                uniqueIds.Add(localTypeIds[s]);
                _indexLists.Add(byType[localTypeIds[s]]);
            }
        }
        _typeIds = uniqueIds.ToArray();

        // keyed 二级字典（批次一）: 仅收录 Key != null 的 keyed 注册（默认注册不进
        // 此结构，GetKeyedService(type, null) 落回默认注册域）。单趟构建：去重开组 +
        // 索引器 upsert，同 (type,key) 后注册覆盖先注册，"最后注册优先"由写入顺序
        // 自然成立。
        _keyedLookup = new Dictionary<int, Dictionary<int, int>>();
        for (int j = 0; j < descriptors.Count; j++)
        {
            var descKey = descriptors[j].Key;
            if (descKey != null)
            {
                int typeVal = descriptors[j].ServiceType.TypeId;
                if (!_keyedLookup.ContainsKey(typeVal))
                {
                    _keyedLookup.Add(typeVal, new Dictionary<int, int>());
                }
                string keyStr = (string)descKey;
                int keyHash = keyStr.GetHashCode();
                _keyedLookup[typeVal][keyHash] = j;
            }
        }

        // Build 时预构造所有 Singleton——单线程内完成，消除运行时 check-then-act 竞态，
        // 并在构造链中检测循环依赖（含环路径异常），兑现 header 的"零竞态、零锁"承诺。
        _constructing = new List<int>();
        this.PreConstructSingletons();
    }

    /// Build 时预构造所有 Singleton 实例。
    ///
    /// 并发安全核心: 运行时 _singletons 为不可变只读列表，
    /// GetService 仅做列表读取——零锁、零竞态。
    ///
    /// 循环依赖检测: 构造期间维护"正在构造"栈，
    /// 若递归解析回到自身则抛 InvalidOperationException（含环路径）。
    private void PreConstructSingletons() {
        for (int i = 0; i < _descriptors.Count; i++) {
            var desc = _descriptors[i];
            if (desc.Lifetime == ServiceLifetime.Singleton) {
                this.ConstructSingleton(i);
            }
        }
    }

    /// 构造单个 Singleton 实例，带循环依赖检测。
    ///
    /// 唯一的 Singleton 构造入口：GetService/GetKeyedService/GetServices 的
    /// Singleton 分支一律经此方法——保证循环依赖检测对递归解析路径（工厂
    /// 内部经 GetService 解析依赖）同样生效，而非仅在预构造顶层循环检测。
    private void ConstructSingleton(int index) {
        if (_singletons[index] != null) {
            return;  // 已构造（预构造完成或被依赖链提前构造）
        }
        // 循环依赖检测：index 已在构造栈中 → 形成环，报含环路径的异常。
        for (int c = 0; c < _constructing.Count; c++) {
            if (_constructing[c] == index) {
                throw new InvalidOperationException(this.BuildCircularMessage(index, c));
            }
        }

        var desc = _descriptors[index];
        var fac = desc.Factory;
        if (fac == null) {
            throw new InvalidOperationException(
                "Service factory is null for: " + desc.ServiceType.FullName +
                ". Ensure codegen injected the factory.");
        }

        // 工厂调用时传入 this——工厂内部递归解析依赖，若依赖另一个未构造的
        // Singleton 会经 GetService 递归进入 ConstructSingleton，环在此检测。
        _constructing.Add(index);
        _singletons[index] = fac(this);
        _constructing.RemoveAt(_constructing.Count - 1);
    }

    /// 组装循环依赖异常消息：自环起点串联构造栈直至回到起点，形如 A -> B -> A。
    private string BuildCircularMessage(int index, int firstOccurrence) {
        string path = _descriptors[index].ServiceType.FullName;
        for (int c = firstOccurrence + 1; c < _constructing.Count; c++) {
            path = path + " -> " + _descriptors[_constructing[c]].ServiceType.FullName;
        }
        path = path + " -> " + _descriptors[index].ServiceType.FullName;
        return "Circular dependency detected: " + path;
    }

    /// 内置服务解析——ServiceProvider 自动注册自身为 IServiceScopeFactory / IServiceProvider。
    /// 对齐 MEDI 语义：根容器无需用户手动注册即可解析这些基础设施接口。
    private object? GetBuiltInService(Type serviceType) {
        if (serviceType.TypeId == typeof(IServiceScopeFactory).TypeId) {
            return this;
        }
        if (serviceType.TypeId == typeof(Arc.IServiceProvider).TypeId) {
            return this;
        }
        return null;
    }

    /// <summary>二分定位类型的注册索引列表（升序唯一 type_id 数组，O(log n)、无哈希、无装箱）。</summary>
    /// <param name="typeId">服务类型 TypeId。</param>
    /// <returns>该类型按注册顺序的索引列表；未注册返回 null。</returns>
    private List<int>? FindIndices(int typeId)
    {
        // 空容器守卫：无注册时 _typeIds 为空（长度 0 或 null 哨兵），直接判未注册，
        // 不进入 Array.BinarySearch，保证空 Provider 解析语义正确（返回 null、不崩溃）。
        if (_typeIds == null || _typeIds.Length == 0)
        {
            return null;
        }
        int pos = Array.BinarySearch(_typeIds, typeId);
        if (pos < 0)
        {
            return null;
        }
        return _indexLists[pos];
    }

    public object? GetService(Type serviceType)
    {
        if (_disposed)
        {
            throw new ObjectDisposedException("ServiceProvider");
        }
        // 内置服务——ServiceProvider 自身即为 IServiceScopeFactory / IServiceProvider
        // （MEDI 语义：根容器自动注册自身，无需用户手动 Add）
        var builtIn = this.GetBuiltInService(serviceType);
        if (builtIn != null) { return builtIn; }

        var indices = this.FindIndices(serviceType.TypeId);
        if (indices == null) { return null; }

        // 最后注册优先（对齐 .NET：同名服务后注册覆盖）。逆序遍历索引列表，
        // 首个 Key==null 的注册即为最后注册项。
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
                    this.ConstructSingleton(i);
                    // CD-29 根因：容器缓存引用（_singletons 持有）不得裸返回——Arc
                    // codegen 的返回值是「借用」（无 inc），命名局部却按「新引用」dec
                    //（epilogue）。裸返回缓存会令每次调用方 dec 使缓存对象计数漂移 →
                    // 提前 free → 悬垂 dec（free DUP）→ UAF（web_core_auth_concurrency
                    // 随机失败根因，ARC_DBG_FREE 实证）。经中间局部赋值（inc）把借用转
                    // 新引用：调用方 dec 与容器持有各自独立，生命周期平衡。
                    object? instance = _singletons[i];
                    return instance;
                }
                // MEDI 语义: 根容器不能解析 Scoped 服务
                if (lt == ServiceLifetime.Scoped) {
                    throw new InvalidOperationException(
                        "Cannot resolve scoped service from root provider. " +
                        "Use IServiceScope.");
                }
                // Transient: 每次解析新建
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
            throw new ObjectDisposedException("ServiceProvider");
        }
        // null key 落入默认注册域——对齐 MEDI（GetKeyedService(type, null) 与
        // GetService 同域解析），keyed 字典不收录默认注册，避免双处存储。
        if (key == null)
        {
            return this.GetService(serviceType);
        }

        // keyed O(1) 字典化主路径（批次一）: 外键 type_id、内键 key 的确定性
        // 内容哈希；TryGetValue 区分"未注册"与"索引 0"（索引器 get 未命中
        // 返回 default(int)=0，与合法索引 0 有歧义，禁用）。
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
            this.ConstructSingleton(i);
            // CD-29（同 GetService）：容器缓存引用经中间局部赋值转新引用，
            // 避免调用方 dec 使 Singleton 计数漂移（提前 free → UAF）。
            object? instance = _singletons[i];
            return instance;
        }
        if (lt == ServiceLifetime.Scoped)
        {
            throw new InvalidOperationException(
                "Cannot resolve keyed scoped service from root provider. " +
                "Use IServiceScope.");
        }
        return fac(this);
    }

    /// <summary>解析某类型的全部 default 注册实例（按注册顺序）；无注册返回空列表。</summary>
    /// <remarks>
    /// 对齐 .NET IServiceProvider 的 IEnumerable&lt;T&gt; 解析。ServiceProvider（根）语义与
    /// GetService 一致：Scoped 注册从根解析抛异常（须经 IServiceScope）。
    /// </remarks>
    public List<object?> GetServices(Type serviceType)
    {
        if (_disposed)
        {
            throw new ObjectDisposedException("ServiceProvider");
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
                    this.ConstructSingleton(i);
                    // CD-29（同 GetService）：缓存引用经中间局部转新引用，
                    // 列表元素 dec 与容器持有独立平衡。
                    object? item = _singletons[i];
                    results.Add(item);
                } else if (lt == ServiceLifetime.Scoped) {
                    throw new InvalidOperationException(
                        "Cannot resolve scoped service from root provider. " +
                        "Use IServiceScope.");
                } else {
                    results.Add(fac(this));
                }
            }
            k = k + 1;
        }
        return results;
    }

    public IServiceScope CreateScope()
    {
        if (_disposed) { throw new ObjectDisposedException("ServiceProvider"); }
        // 共享根容器 Build 后的不可变查找结构与 keyed 字典——作用域零构建开销，
        // 与 _descriptors/_singletons 同为快照语义。
        return new ServiceScope(this, _descriptors, _typeIds, _indexLists, _keyedLookup);
    }

    public void Dispose() {
        if (_disposed) { return; }
        _disposed = true;

        // 逆序释放 Singleton 实例，级联 IDisposable
        // 异常安全: 单个 Dispose 抛异常不中断剩余释放
        var i = _singletons.Count - 1;
        while (i >= 0) {
            var instance = _singletons[i];
            if (instance != null) {
                if (instance is IDisposable) {
                    var d = (IDisposable)instance;
                    d.Dispose();
                }
                _singletons[i] = null;
            }
            i = i - 1;
        }

        _singletons = null;
        _constructing = null;
        _descriptors = null;
        _typeIds = null;
        _indexLists = null;
        _keyedLookup = null;
    }
}
