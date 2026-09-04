namespace Arc.Runtime;

// ============================================================
// AssemblyPackageMeta —— 包元数据（RFC 017 M4）
// ============================================================

/// 动态库的包身份信息，从 arc.toml [package] 节提取并嵌入
/// __arc_package_meta 全局符号。对齐 C# AssemblyName 概念。
public struct AssemblyPackageMeta
{
    /// 包名（arc.toml [package].name）。
    public string Name;

    /// 包版本（arc.toml [package].version，如 "1.0.0"）。
    public string Version;

    /// 语言版本（arc.toml [package].edition，如 "1"）。
    public string Edition;

    /// 运行时传递依赖（RFC 017 M3 gap ②）：`[dependencies]` 中 path 依赖的包名。
    /// 元数据未携带依赖时为空列表（非 null）。
    public List<string> Dependencies;

    /// 布局指纹表（RFC 045 D8.1 状态迁移 L1）：原始 `Type:sig` 段列表
    /// （`__arc_package_meta` 第 5 字段的 `;` 分段原文）。刻意用 List&lt;string&gt;
    /// 而非 Dictionary&lt;string,long&gt;——值类型值参数化的字典泛型实例化在
    /// std 编译单元内无先例（实证触发加载期崩溃），段解析惰性下沉到
    /// `GetLayoutSignature`（单值 TryParse，YamlParser 同源非 panic 先例）。
    /// 旧产物未携带此段时为空列表（非 null）。
    public List<string> LayoutSigs;

    /// 全字段构造。含引用类型字段的 struct 须经构造器分配——`default(本类型)`
    /// 在 codegen 中退化为空指针，随后逐字段赋值会写空指针（AV）。
    public AssemblyPackageMeta(string name, string version, string edition)
    {
        this.Name = name;
        this.Version = version;
        this.Edition = edition;
        this.Dependencies = new List<string>();
        this.LayoutSigs = new List<string>();
    }

    /// 未携带包元数据时返回 true。
    public bool IsEmpty
    {
        get { return this.Name == null || this.Name.Length == 0; }
    }

    /// 友好格式：Name, Version=x.x.x, Edition=x
    public string ToString()
    {
        if (this.IsEmpty) { return "<no package metadata>"; }
        return this.Name + ", Version=" + this.Version +
               ", Edition=" + this.Edition;
    }
}

// ============================================================
// Assembly —— 程序集表示（RFC 017 M3）
// ============================================================

/// 程序集——一个已加载的动态库实例。
///
/// 对齐 C# System.Reflection.Assembly，提供：
/// - Name：库文件名
/// - PackageMeta：包元数据（来自 arc.toml [package]）
/// - Entry<T>：泛型入口调用
/// - Dispose：资源释放
///
/// ## 泛型 Entry 方法
///
///   Entry<TResult>()                     // 无参入口
///   Entry<TParameter, TResult>(TParameter? args)  // 单参入口
///
/// **任意类型支持**——不对 TParameter/TResult 设 struct 约束：
///   - struct 类型：codegen wrapper 按值 marshal（memcpy）
///   - class 类型：ArcHeader* 透传（零包装，零装箱）
///   - 可空：null 参数传 NULL 指针，null 返回传 NULL 指针
///
/// ## 资源管理
///
/// - IDisposable + using 块自动释放
/// - _disposed 守卫：释放后调用抛 ObjectDisposedException
/// - Cold unload（Assembly.Dispose() → rt_library_unload）保留（RFC 017）
/// - Hot unload（AssemblyLoadContext.Unload() → 回收协议）后，本实例保留句柄
///   供卸载后访问触发 E_UNLOAD_HANGING_REF 硬错误（RFC 017 §2.4）
public class Assembly
{
    private NativePtr _handle;
    private string _name;
    private AssemblyPackageMeta _packageMeta;
    private bool _packageMetaLoaded;
    private bool _disposed;
    private bool _unloaded;
    private int _generation;

    /// 获取当前正在执行的程序集（RFC 017 M1 收尾）。
    ///
    /// 返回 <c>AssemblyLoadContext.Load</c> 最近设置的当前程序集（C 运行时
    /// <c>rt_assembly_set_executing</c> 记录的 <c>Assembly*</c> 对象指针）。
    /// 未设置返回 null。返回值为 ALC <c>_loaded</c> 持有的同一实例（借用引用，
    /// 不额外 inc/dec）；ALC 卸载后指针可能悬垂——调用方应在模块生命周期内使用。
    public static Assembly? GetExecutingAssembly()
    {
        NativePtr p = rt_library.rt_assembly_get_executing();
        if (p == null) { return null; }
        return (Assembly)p;
    }

    /// 内部构造——仅 AssemblyLoadContext 创建实例。
    internal Assembly(NativePtr handle, string name)
    {
        _handle = handle;
        _name = name;
        _packageMeta = default(AssemblyPackageMeta);
        _packageMetaLoaded = false;
        _disposed = false;
        _unloaded = false;
        _generation = rt_library.rt_library_generation(handle);
    }

    /// 库标识名（`Load` 传入的 resolvedPath 完整路径——`_loaded` 登记键、
    /// `GetLoadedAssembly`/依赖声明匹配均以此为准；`LoadByName` 场景为
    /// 探针解析后的绝对路径）。非短文件名。
    public string Name { get { return _name; } }

    /// 底层动态库句柄（内部使用）。
    internal NativePtr Handle { get { return _handle; } }

    /// 模块代数（RFC 017 §2.2）：同路径重复加载获得新代数；tombstone 后为 0。
    internal int Generation { get { return _generation; } }

    /// 是否已被 AssemblyLoadContext.Unload() 热卸载（句柄保留供悬垂检测）。
    internal bool IsUnloaded { get { return _unloaded; } }

    /// 是否已释放（冷卸载路径 Dispose() 置位）。
    internal bool IsDisposed { get { return _disposed; } }

    /// 包元数据——从 arc.toml [package] 节提取的 name/version/edition。
    /// 首次访问时通过 rt_library_get_meta_field 懒加载解析。
    /// 库未携带元数据时返回 IsEmpty == true 的默认值。
    public AssemblyPackageMeta PackageMeta
    {
        get
        {
            if (!_packageMetaLoaded && _handle != null) {
                _packageMeta = ParsePackageMeta(_handle);
                _packageMetaLoaded = true;
            }
            return _packageMeta;
        }
    }

    /// 包全名：Name, Version=x.x.x, Edition=x（对齐 C# Assembly.FullName）。
    public string FullName
    {
        get { return this.PackageMeta.ToString(); }
    }

    /// 读取类型布局指纹（RFC 045 D8.1 状态迁移 L1）。
    ///
    /// 返回该程序集编译时 `entry_layout_signature`（FNV-1a-64 布局传递闭包）
    /// 的物化值；返回 0 = 未物化（旧产物无此段 / 类型不在表 / 名字为空）——
    /// 跨代兼容性判定按**保守拒绝**处理（未知 ≠ 兼容）。枚举 / variant /
    /// 基元不物化（枚举布局恒为判别值宽度、variant 无字段布局），查表即 0。
    /// 段解析惰性执行（本方法内逐段匹配，非 panic 的 long.TryParse）。
    public long GetLayoutSignature(string typeName)
    {
        if (typeName == null || typeName.Length == 0) { return 0; }
        AssemblyPackageMeta meta = this.PackageMeta;
        List<string> segs = meta.LayoutSigs;
        if (segs == null) { return 0; }
        for (int i = 0; i < segs.Count; i++)
        {
            string seg = segs[i];
            int colon = seg.IndexOf(":");
            if (colon <= 0 || colon >= seg.Length - 1) { continue; }
            string candidate = seg.Substring(0, colon);
            if (candidate != typeName) { continue; }
            string sigText = seg.Substring(colon + 1, seg.Length - colon - 1);
            long sig = 0;
            if (!long.TryParse(sigText, ref sig)) { return 0; }
            return sig;
        }
        return 0;
    }

    // ---- 内部帮助方法 ----

    /// 解析 "name\0version\0edition\0[dep1\0dep2\0...]" 格式的包元数据。
    ///
    /// 逐字段经 rt_library_get_meta_field 按索引读取（0=name、1=version、
    /// 2=edition）。Arc `string` 为纯 C-string——整串会在首个 NUL 处截断，
    /// 旧实现经 `IndexOf('\0')` 拆分只能读到 name，version/edition 恒为空
    /// （RFC 017 M4 既有损坏）。C 侧按 NUL 分段规避该截断。
    /// 依赖字段从索引 3 起逐个读取，直到空串/NULL（NUL 段终止符）。
    private static AssemblyPackageMeta ParsePackageMeta(NativePtr handle)
    {
        if (handle == null) {
            return default(AssemblyPackageMeta);
        }

        string name = rt_library.rt_library_get_meta_field(handle, 0);
        if (name == null || name.Length == 0) {
            return default(AssemblyPackageMeta);
        }

        string version = rt_library.rt_library_get_meta_field(handle, 1);
        string edition = rt_library.rt_library_get_meta_field(handle, 2);
        AssemblyPackageMeta meta = new AssemblyPackageMeta(
            name,
            version != null ? version : "",
            edition != null ? edition : "");

        // RFC 017 M3 gap ②：索引 3 起为传递依赖名，逐项读取直至空串/NULL
        // （双 NUL 终止符）。直接追加到构造器分配的 Dependencies 列表。
        // 布局指纹子表（RFC 045 D8.1 状态迁移 L1）以 `#layouts:` 自描述
        // 前缀内嵌于依赖流——依赖循环遇前缀字段即转入指纹解析并终止
        // （后续无其他业务字段）。子表 `Type1:sig1;...`（';' 分段、':' 分
        // 名值）。解析全程非 panic：char 索引与 long.Parse 均有 rt_panic
        // 风险（其失败不可捕获、中止整个程序，见 YamlParser 同源先例），
        // 改用 Substring 比较 + 非 panic 的 long.TryParse；非法段宽容跳过。
        int index = 3;
        while (true) {
            string dep = rt_library.rt_library_get_meta_field(handle, index);
            if (dep == null || dep.Length == 0) { break; }
            if (dep.IndexOf("#layouts:") == 0) {
                string rest = dep.Substring(9, dep.Length - 9);
                while (rest.Length > 0) {
                    int semi = rest.IndexOf(";");
                    string seg = semi < 0 ? rest : rest.Substring(0, semi);
                    rest = semi < 0 ? "" : rest.Substring(semi + 1, rest.Length - semi - 1);
                    if (seg.Length == 0) { continue; }
                    int colon = seg.IndexOf(":");
                    if (colon <= 0 || colon >= seg.Length - 1) { continue; }
                    string typeName = seg.Substring(0, colon);
                    string sigText = seg.Substring(colon + 1, seg.Length - colon - 1);
                    long sig = 0;
                    if (!long.TryParse(sigText, ref sig)) { continue; }
                    meta.LayoutSigs.Add(seg);
                }
                break;
            }
            meta.Dependencies.Add(dep);
            index++;
        }
        return meta;
    }

    // ========== 泛型 Entry 入口 ==========

    /// 调用无参 Entry<TResult>() 入口。
    /// 符号名：__arc_entry__{TR_id}
    ///
    /// codegen 在调用点拦截（RFC 017 M2「Entry&lt;T&gt; 确定性契约」）：按方法身份
    /// （接收者静态类型 Assembly + 方法名 Entry + 参数个数）识别后，单态化计算
    /// 符号名，经 rt_library_sym 解析函数指针并以统一 void*→void* 零装箱 C ABI
    /// 间接调用；NULL → EntryPointNotFoundException。本方法体为 dead-code facade，
    /// 仅保留签名与默认返回占位，运行时不会执行。
    public TResult? Entry<TResult>()
    {
        return default(TResult?);
    }

    /// 调用单参 Entry<TParameter, TResult>(TParameter? args) 入口。
    /// 符号名：__arc_entry_{TP_id}_{TR_id}
    ///
    /// 与无参形态同源：codegen 在调用点拦截（RFC 017 M2「Entry&lt;T&gt; 确定性
    /// 契约」），本方法体为 dead-code facade，仅保留签名与默认返回占位，运行时
    /// 不会执行。
    public TResult? Entry<TParameter, TResult>(TParameter? args)
    {
        return default(TResult?);
    }

    /// 查找模块内符号（C 函数/数据指针）。
    /// 模块已卸载时访问 → E_UNLOAD_HANGING_REF 硬错误（由 C 运行时
    /// tombstone 检测触发，RFC 017 §2.4）。
    public NativePtr ResolveSymbol(string name)
    {
        this.ThrowIfDisposed();
        if (name == null || name.Length == 0) { return null; }
        return rt_library.rt_library_sym(_handle, name);
    }

    // ---- Entry 内部方法 ----

    private void ThrowIfDisposed()
    {
        if (_disposed) {
            throw new ObjectDisposedException("Assembly");
        }
    }

    private NativePtr LookupEntry(string symbolName)
    {
        NativePtr fn = rt_library.rt_library_sym(_handle, symbolName);
        if (fn == null) {
            throw new EntryPointNotFoundException(
                "Entry point not found in '" + _name +
                "': " + symbolName);
        }
        return fn;
    }

    // ========== 资源释放 ==========

    /// 标记已热卸载（AssemblyLoadContext.Unload() 成功路径调用）。
    /// 句柄保留——卸载后访问经 C 运行时 tombstone 检测触发
    /// E_UNLOAD_HANGING_REF 硬错误。
    internal void MarkUnloaded()
    {
        _unloaded = true;
    }

    /// 释放动态库句柄（冷卸载路径，RFC 017 保留）。
    /// 已热卸载的模块再次 Dispose 为幂等 no-op（C 运行时 tombstone 判定）。
    public void Dispose()
    {
        if (!_disposed && _handle != null) {
            rt_library.rt_library_unload(_handle);
            _handle = null;
            _disposed = true;
        }
    }
}
