namespace Arc.Runtime;

using Arc.IO;

// ============================================================
// AssemblyLoadContext —— 动态库加载上下文（RFC 017 M4）
// ============================================================

/// 动态库加载上下文——对齐 C# System.Runtime.Loader.AssemblyLoadContext。
///
/// 底层通过 crates/arc/native/rt_library.ani FFI 契约调用 C 运行时加载/卸载动态库。
///
/// ## 可回收 ALC（RFC 017 热卸载闭环）
///
/// `Unload()` 触发回收协议（Freeze → 在途收敛 → 归零检测 → 释放根 →
/// dlclose → tombstone）：
/// - 模块级代数引用计数：`HoldReference(asm)` / `ReleaseReference(asm)`
///   登记跨模块外部强引用；非零 → 卸载被拒（`E_UNLOAD_HANGING_REF`）。
/// - ARC 根扫描：`RegisterModuleRoot` / `CanUnload` 判定无模块外强引用。
/// - 卸载后访问已卸载符号 → `E_UNLOAD_HANGING_REF` 硬错误（禁静默）。
///
/// ## 加载模式
///
/// ### 1. 直接路径加载（当前完整支持）
///
///   var asm = alc.Load("plugins/myplugin.dll");
///
/// ### 2. 按名称加载 + 探针路径（当前完整支持）
///
///   alc.AddProbingPath("./plugins");
///   alc.AddProbingPath("./lib");
///   var asm = alc.LoadByName("myplugin");  // 在探针路径中搜索 myplugin.dll
///
/// ### 3. 传递性依赖自动加载（RFC 017 M3 gap ②）
///
///   库的 __arc_package_meta 嵌入依赖列表（`[dependencies]` 的 path 依赖键，
///   经 `AssemblyPackageMeta.Dependencies` 暴露）。Load() 读取依赖列表并经
///   探针路径递归加载（requestingAssembly = 本程序集，已加载路径跳过防环）。
///
/// ## 探针路径
///
///   默认探针路径：入口程序集所在目录、./lib/。
///   AddProbingPath() 追加自定义路径；按添加顺序搜索。
///
/// ## 依赖关系追踪
///
///   _loaded: 已加载程序集字典 (name → Assembly)
///   _dependencyGraph: 依赖图 (loaded → requestingAssembly)
///   GetDependencies(name) 查询反向依赖链
///
/// ## C# 一致的表面体验
///
///   var alc = AssemblyLoadContext.Default;
///   alc.AddProbingPath("./plugins");
///   alc.SetLifecycle(new MyLifecycle());
///
///   // 直接路径
///   var asm = alc.Load("plugins/myplugin.dll");
///
///   // 或按名称 + 探针
///   var asm = alc.LoadByName("myplugin");
///
///   Console.WriteLine(asm.PackageMeta.Name);    // "myplugin"
///   Console.WriteLine(asm.PackageMeta.Version); // "1.0.0"
///
///   var result = asm.Entry<Config, Output>(config);
public class AssemblyLoadContext
{
    // ---- 单例 ----
    // static readonly 惰性（RFC 006：静态成员）：首触构造一次、线程安全，替代手写 `_default == null` 缓存。
    public static readonly AssemblyLoadContext Default = new AssemblyLoadContext();

    // ---- 状态 ----

    private DefaultAssemblyLifecycle _lifecycle;
    private Dictionary<string, Assembly> _loaded;
    private Dictionary<string, string> _dependencyGraph;
    private List<string> _probingPaths;

    private AssemblyLoadContext()
    {
        _lifecycle = new DefaultAssemblyLifecycle();
        _loaded = new Dictionary<string, Assembly>();
        _dependencyGraph = new Dictionary<string, string>();
        _probingPaths = new List<string>();

        // 默认探针路径：当前目录 + ./lib/
        _probingPaths.Add(".");
        _probingPaths.Add("./lib");
    }

    // ========== 流式 API ==========

    /// 注入生命周期实例（RFC 017）。
    ///
    /// 编译器限制：std 侧对抽象类字段的存储/分派无法正确 codegen，故以
    /// 具体类 `DefaultAssemblyLifecycle` 承载钩子；自定义生命周期派生该类、
    /// 由入口包构造实例注入。返回 this，支持链式调用。
    public AssemblyLoadContext SetLifecycle(DefaultAssemblyLifecycle lifecycle)
    {
        _lifecycle = lifecycle;
        return this;
    }

    // ========== 探针路径 ==========

    /// 追加探针路径。LoadByName() 按添加顺序搜索。
    /// 路径支持相对路径（相对于当前工作目录）或绝对路径。
    public AssemblyLoadContext AddProbingPath(string path)
    {
        if (path != null && path.Length > 0) {
            _probingPaths.Add(path);
        }
        return this;
    }

    /// 返回所有探针路径的副本。
    public List<string> GetProbingPaths()
    {
        var result = new List<string>();
        for (int i = 0; i < _probingPaths.Count; i++) {
            result.Add(_probingPaths[i]);
        }
        return result;
    }

    /// 在探针路径中按名称搜索库文件。
    /// 返回第一个匹配文件的完整路径；未找到返回 null。
    public string? ResolveLibrary(string name)
    {
        return this.ResolveLibraryWithBase(name, null);
    }

    /// 在探针路径中搜索，优先从 requestingAssembly 所在目录开始。
    private string? ResolveLibraryWithBase(string name, Assembly? requestingAssembly)
    {
        // 优先级 1: 请求方所在目录
        if (requestingAssembly != null) {
            string? baseDir = GetDirectoryName(requestingAssembly.Name);
            if (baseDir != null) {
                string? found = TryFindLibrary(name, baseDir);
                if (found != null) { return found; }
            }
        }

        // 优先级 2: 探针路径（按添加顺序）
        for (int i = 0; i < _probingPaths.Count; i++) {
            string? found = TryFindLibrary(name, _probingPaths[i]);
            if (found != null) { return found; }
        }

        return null;
    }

    /// 在指定目录中查找库文件（尝试各平台扩展名）。
    private static string? TryFindLibrary(string name, string directory)
    {
        // 按优先级尝试平台扩展名
        // Windows: .dll 优先；Linux: .so；macOS: .dylib
        string[] extensions = GetPlatformLibraryExtensions();
        for (int i = 0; i < extensions.Length; i++) {
            string candidate = directory + "/" + name + extensions[i];
            if (File.Exists(candidate)) {
                return candidate;
            }
        }

        // 也尝试无扩展名（某些平台约定）
        string bare = directory + "/" + name;
        if (File.Exists(bare)) {
            return bare;
        }

        return null;
    }

    /// 返回当前平台的动态库扩展名列表（按优先级）。
    private static string[] GetPlatformLibraryExtensions()
    {
        if (Environment.IsWindows()) {
            return [".dll"];
        } else if (Environment.IsMacOS()) {
            return [".dylib", ".so"];
        } else {
            // Linux / OHos / others
            return [".so"];
        }
    }

    /// 从完整路径提取目录部分。
    private static string? GetDirectoryName(string path)
    {
        if (path == null) { return null; }

        int lastSep = -1;
        for (int i = path.Length - 1; i >= 0; i--) {
            if (path[i] == '/' || path[i] == '\\') {
                lastSep = i;
                break;
            }
        }
        if (lastSep < 0) { return "."; }
        return path.Substring(0, lastSep);
    }

    // ========== 加载 ==========

    /// 通过探针路径按名称加载库。
    /// 等价于 ResolveLibrary(name) → Load(resolvedPath)。
    public Assembly LoadByName(string name, Assembly? requestingAssembly = null)
    {
        string? resolvedPath = this.ResolveLibraryWithBase(name, requestingAssembly);
        if (resolvedPath == null) {
            throw new IOException(
                "Library not found in probing paths: " + name +
                ". Use AddProbingPath() to register additional search directories.");
        }
        return this.Load(resolvedPath, requestingAssembly);
    }

    /// 通过路径加载动态库，返回 Assembly 句柄。
    ///
    /// 加载流程：
    ///   1. OnResolving → 路径解析（可改写路径、触发依赖发现）
    ///   2. rt_library_load → 底层动态库加载
    ///   3. 构造 Assembly 实例（PackageMeta 懒加载）
    ///   4. OnLoaded → 生命周期通知（可在钩子中做版本校验、触发依赖加载）
    ///
    /// requestingAssembly: 触发加载的请求方；null = 顶层调用。
    public Assembly Load(string path, Assembly? requestingAssembly = null)
    {
        // 1. 解析阶段
        var resolveArgs = new AssemblyResolvingArgs(this, path, requestingAssembly);
        DefaultAssemblyLifecycle lc = _lifecycle;
        string? resolvedPath = lc.OnResolving(resolveArgs);
        if (resolvedPath == null || resolvedPath.Length == 0) {
            throw new IOException("Failed to resolve library: " + path);
        }

        // 2. 实际加载
        NativePtr handle = rt_library.rt_library_load(resolvedPath);
        if (handle == null) {
            throw new IOException("Failed to load library: " + resolvedPath);
        }

        // 3. 构造 Assembly（PackageMeta 通过属性懒加载——首次访问时
        //    调用 rt_library_get_meta 解析 __arc_package_meta 全局符号）
        Assembly asm = new Assembly(handle, resolvedPath);
        // 属性读取先落本地（MIR 属性拦截仅走语句路径；字典索引 operand 直降
        // operand_from_expr 的 Field 读，会按属性名当字段读而类型错配）
        string asmName = asm.Name;
        _loaded[asmName] = asm;

        // RFC 017 M1：将本 Assembly 记为「当前执行的程序集」——C 运行时保存
        // `Assembly*` 对象指针（裸指针，不 inc/dec；对象由 `_loaded` 保持存活）。
        // `Assembly.GetExecutingAssembly()` 经 `rt_assembly_get_executing` 读回。
        rt_library.rt_assembly_set_executing((NativePtr)asm);

        if (requestingAssembly != null) {
            _dependencyGraph[asmName] = requestingAssembly.Name;
        }

        // 4. 加载通知——业务方可在 OnLoaded 钩子中：
        //    - 访问 asm.PackageMeta 获取包身份（name/version/edition）
        //    - 执行版本兼容性校验
        // 传递依赖的自动加载在步骤 5 完成（RFC 017 M3 gap ②）。
        var loadArgs = new AssemblyLoadArgs(this, resolvedPath, asm,
                                             requestingAssembly);
        _lifecycle.OnLoaded(loadArgs);

        // 5. 传递依赖自动加载（RFC 017 M3 gap ②）：读取嵌入的依赖列表，
        //    经探针路径按名称递归加载，requestingAssembly = 本程序集。
        this.LoadDependencies(asm);
        return asm;
    }

    /// 递归加载 <paramref name="assembly"/> 声明的运行时依赖（RFC 017 M3 gap ②）。
    ///
    /// 依赖名经 <see cref="ResolveLibraryWithBase"/> 按探针路径解析（优先请求方
    /// 所在目录），镜像 <see cref="LoadByName"/> 的探针语义；已加载路径跳过以
    /// 防止循环依赖。解析失败抛 <see cref="IOException"/>（与 LoadByName 一致）。
    private void LoadDependencies(Assembly assembly)
    {
        AssemblyPackageMeta meta = assembly.PackageMeta;
        if (meta.Dependencies == null) { return; }

        for (int i = 0; i < meta.Dependencies.Count; i++)
        {
            string depName = meta.Dependencies[i];
            if (depName == null || depName.Length == 0) { continue; }

            string? resolvedPath = this.ResolveLibraryWithBase(depName, assembly);
            if (resolvedPath == null)
            {
                throw new IOException(
                    "Dependency not found in probing paths: " + depName +
                    ". Use AddProbingPath() to register additional search directories.");
            }
            if (_loaded.ContainsKey(resolvedPath)) { continue; }
            this.Load(resolvedPath, assembly);
        }
    }

    /// 从目录批量加载动态库（RFC 017 M3 收尾）。
    ///
    /// 按平台库扩展名枚举目录下的动态库文件（<c>Directory.GetFiles(path, "*.ext")</c>，
    /// 完整路径 string[]），逐个经 <see cref="Load(string, Assembly)"/> 加载并返回
    /// 已加载 <see cref="Assembly"/> 列表。已加载路径去重（跨扩展名探测与重复调用幂等）。
    /// 非库文件（如 .txt/.arcgr/.arcdbg）不尝试加载；加载失败（<c>rt_library_load</c>
    /// 返回 null）由 <see cref="Load"/> 抛出 <see cref="IOException"/>，不静默吞错。
    public List<Assembly> LoadFromDirectory(string directory)
    {
        var result = new List<Assembly>();
        string[] extensions = GetPlatformLibraryExtensions();
        for (int e = 0; e < extensions.Length; e++)
        {
            string[] files = Directory.GetFiles(directory, "*" + extensions[e]);
            for (int i = 0; i < files.Length; i++)
            {
                if (this.GetLoadedAssembly(files[i]) != null)
                {
                    continue;
                }
                result.Add(this.Load(files[i]));
            }
        }
        return result;
    }

    // ========== 查询 ==========

    public Assembly? GetLoadedAssembly(string name)
    {
        if (_loaded.ContainsKey(name)) {
            return _loaded[name];
        }
        return null;
    }

    public string? GetLoadedBy(string name)
    {
        if (_dependencyGraph.ContainsKey(name)) {
            return _dependencyGraph[name];
        }
        return null;
    }

    /// 查询某个程序集被哪些程序集依赖（反向依赖链）。
    public List<string> GetDependencies(string name)
    {
        var result = new List<string>();
        string[] rawKeys = _dependencyGraph.Keys;
        for (int i = 0; i < rawKeys.Length; i++) {
            string loaded = rawKeys[i];
            if (_dependencyGraph[loaded] == name) {
                result.Add(loaded);
            }
        }
        return result;
    }

    /// 所有已加载的程序集名称列表。
    public List<string> GetLoadedAssemblies()
    {
        var result = new List<string>();
        string[] rawKeys = _loaded.Keys;
        for (int i = 0; i < rawKeys.Length; i++) {
            result.Add(rawKeys[i]);
        }
        return result;
    }

    // ========== 卸载 ==========

    /// 反查在载依赖方：`Dependencies` 声明含 target 包名的已加载模块
    /// （按包名匹配——依赖声明与 PackageMeta.Name 同域）。返回空列表 =
    /// 无依赖方在载（可安全卸载）。target 无包元数据时恒为空——依赖按
    /// 包名声明，无元数据则无从匹配（护栏盲区，须以发布约定兜底）。
    ///
    /// 数据源取元数据声明而非 `_dependencyGraph`：后者只记录加载触发
    /// 关系（requestingAssembly），不覆盖「A 直接 Load(D) 但 D 声明依赖
    /// B」的静态依赖边。
    private List<string> FindLoadedDependents(Assembly target)
    {
        var result = new List<string>();
        AssemblyPackageMeta targetMeta = target.PackageMeta;
        if (targetMeta.IsEmpty) { return result; }
        string targetPkg = targetMeta.Name;
        string[] names = _loaded.Keys;
        for (int i = 0; i < names.Length; i++)
        {
            Assembly loaded = _loaded[names[i]];
            if (loaded == target || loaded.IsDisposed) { continue; }
            AssemblyPackageMeta meta = loaded.PackageMeta;
            if (meta.IsEmpty || meta.Dependencies == null) { continue; }
            for (int j = 0; j < meta.Dependencies.Count; j++)
            {
                if (meta.Dependencies[j] == targetPkg)
                {
                    result.Add(names[i]);
                    break;
                }
            }
        }
        return result;
    }

    /// 热卸载闭环（RFC 017 §2.4）：Freeze → 在途收敛 → 归零检测 → 释放根 →
    /// dlclose → tombstone。
    ///
    /// - 存在跨模块外部强引用 → 抛 InvalidOperationException
    ///   （E_UNLOAD_HANGING_REF 报告，含引用计数），禁静默卸载。
    /// - 依赖方仍在载（其依赖声明含本模块包名）→ 抛 InvalidOperationException
    ///   （E_UNLOAD_DEPENDED 报告，含依赖方名单），禁静默悬垂——被卸模块的
    ///   类型对象/代码解除映射后，依赖方的接口分派与实例化即访问已卸载内存。
    /// - 模块代码在途执行 → 抛 InvalidOperationException
    ///   （须在无模块代码执行时发起卸载）。
    /// - 已被并发卸载 → 幂等 no-op。
    public void Unload(Assembly assembly)
    {
        if (assembly == null || assembly.IsDisposed) { return; }

        var unloadArgs = new AssemblyUnloadArgs(this, assembly);
        _lifecycle.OnUnloading(unloadArgs);
        if (unloadArgs.Cancel) { return; }

        // 卸载顺序护栏（RFC 017 §2.4 补强）：被依赖感知。依赖方在载时
        // 拒绝卸载——被卸模块的类型对象/代码解除映射后，依赖方的接口
        // 分派与实例化即访问已卸载内存（静默 AV 窗口），故在 rt 层
        // ledger/in-flight 之外补第三道前置检测。
        List<string> dependents = this.FindLoadedDependents(assembly);
        if (dependents.Count > 0)
        {
            string dependentNames = "";
            for (int i = 0; i < dependents.Count; i++)
            {
                dependentNames += dependents[i];
                if (i < dependents.Count - 1) { dependentNames += ", "; }
            }
            throw new InvalidOperationException(
                "E_UNLOAD_DEPENDED: cannot unload module '" + assembly.Name +
                "' — loaded dependent(s): " + dependentNames +
                ". Unload them first (dependents before dependencies).");
        }

        string name = assembly.Name;
        int rc = rt_library.rt_library_unload_hot(assembly.Handle);
        if (rc == 1) {
            // 卸载成功：模块已 dlclose + tombstone。Assembly 保留句柄供
            // 卸载后访问触发 E_UNLOAD_HANGING_REF 硬错误。
            _loaded.Remove(name);
            _dependencyGraph.Remove(name);
            assembly.MarkUnloaded();

            var unloadedArgs = new AssemblyUnloadedArgs(this, name);
            _lifecycle.OnUnloaded(unloadedArgs);
            return;
        }
        if (rc == 0) {
            throw new InvalidOperationException(
                "E_UNLOAD_HANGING_REF: cannot unload module '" + name +
                "' — " + this.GetReferenceCount(assembly) +
                " external strong reference(s) still held. Release them " +
                "(or use Weak<T> for boundary references) before unload.");
        }
        if (rc == -1) {
            throw new InvalidOperationException(
                "Cannot unload module '" + name +
                "': in-flight module code has not converged. Initiate unload " +
                "while no module code is executing.");
        }
        // rc == -2：已被并发卸载（或句柄失效）——幂等 no-op。
        if (_loaded.ContainsKey(name)) {
            _loaded.Remove(name);
            _dependencyGraph.Remove(name);
        }
    }

    // ========== 热卸载闭环 API（RFC 017） ==========

    /// 登记跨模块外部强引用（模块边界点——宿主持模块对象引用时调用）。
    /// 模块卸载要求外部引用归零；非零 → 拒绝卸载并报告。
    public bool HoldReference(Assembly assembly)
    {
        if (assembly == null || assembly.Generation == 0) { return false; }
        return rt_library.rt_library_ref_register(assembly.Generation) != 0;
    }

    /// 释放跨模块外部强引用。返回 false = 计数已为 0（重复释放）或代数无效。
    public bool ReleaseReference(Assembly assembly)
    {
        if (assembly == null || assembly.Generation == 0) { return false; }
        return rt_library.rt_library_ref_unregister(assembly.Generation) != 0;
    }

    /// 查询模块外部强引用计数（0 = 可卸载）。
    public int GetReferenceCount(Assembly assembly)
    {
        if (assembly == null || assembly.Generation == 0) { return 0; }
        return rt_library.rt_library_ref_count(assembly.Generation);
    }

    /// 登记模块根（模块静态持有的 class 引用；卸载前由运行时统一释放）。
    public bool RegisterModuleRoot(Assembly assembly, object root)
    {
        if (assembly == null || root == null || assembly.Generation == 0) {
            return false;
        }
        return rt_library.rt_library_root_add(assembly.Generation, root) != 0;
    }

    /// 移除模块根。返回 false = 未登记或代数无效。
    public bool UnregisterModuleRoot(Assembly assembly, object root)
    {
        if (assembly == null || root == null || assembly.Generation == 0) {
            return false;
        }
        return rt_library.rt_library_root_remove(assembly.Generation, root) != 0;
    }

    /// 卸载前 ARC 根扫描（RFC 017 §2.3）：枚举已登记模块根 + 字段可达遍历 +
    /// ledger 一致性复核。true = 可卸载；false = 存在外部引用，卸载将被拒。
    public bool CanUnload(Assembly assembly)
    {
        if (assembly == null || assembly.Generation == 0) { return false; }
        return rt_library.rt_library_root_scan(assembly.Generation) != 0;
    }

    // ========== RFC 017 §2.6 模块边界弱登记表（宿主侧） ==========

    /// 将弱引用登记为「指向 <paramref name="assembly"/> 模块对象的边界弱引用」
    /// （宿主侧弱登记表）。模块卸载时运行时中和已登记槽位（target 置空）→
    /// 卸载后 <c>TryGet()</c> 确定性返回 null（观察 tombstone 头语义，禁悬垂
    /// 复活）。Weak&lt;T&gt; 不阻止卸载（ledger 不计弱引用）。返回 false =
    /// 模块无效 / 已 Freeze / 登记表满。
    public bool RegisterWeakReference<T>(Assembly assembly, Weak<T> weak)
    {
        if (assembly == null || weak == null || assembly.Generation == 0) {
            return false;
        }
        return rt_library.rt_library_weak_register(
            assembly.Generation, weak.GetWeakSlot()) != 0;
    }

    /// 移除弱引用的模块边界登记（显式解除；弱引用析构亦自动 untrack）。
    /// 返回 false = 未登记 / 模块代数无效。
    public bool UnregisterWeakReference<T>(Assembly assembly, Weak<T> weak)
    {
        if (assembly == null || weak == null || assembly.Generation == 0) {
            return false;
        }
        return rt_library.rt_library_weak_unregister(
            assembly.Generation, weak.GetWeakSlot()) != 0;
    }

    public void UnloadAll()
    {
        // 依赖拓扑序卸载（依赖方先、被依赖方后）：每轮挑一个「无在载
        // 依赖方」的模块卸掉，循环至空。插入序 ≠ 依赖序（递归依赖先插
        // 父模块，如 A→C 递归载 D 后键序为 [C, D]），逆序遍历会被
        // E_UNLOAD_DEPENDED 护栏中断。
        while (_loaded.Count > 0)
        {
            string[] names = _loaded.Keys;
            bool progressed = false;
            for (int i = 0; i < names.Length; i++)
            {
                Assembly candidate = _loaded[names[i]];
                if (candidate == null || candidate.IsDisposed) { continue; }
                if (this.FindLoadedDependents(candidate).Count > 0) { continue; }
                this.Unload(candidate);
                progressed = true;
                break;
            }
            if (!progressed)
            {
                // 环依赖兜底：成员互为依赖方，护栏互锁谁也卸不掉（对齐
                // RFC 017「跨模块环经 ledger 拒载」语义）——终止防死循环，
                // 剩余模块交由调用方按业务序处理。
                break;
            }
        }
    }
}

// ============================================================
// 默认生命周期实现（纯透传）
// ============================================================

/// <summary>
/// 默认生命周期：OnResolving 原样返回请求路径，其余钩子空实现。
///
/// RFC 017 编译器限制：std 侧对抽象类字段的存储/分派无法正确 codegen
/// （vtable 未实体化导致 AV），故默认路径以具体类字段 + virtual 方法承载
/// 生命周期钩子；自定义生命周期通过派生本类（entry 包构造实例 vtable
/// 完整）注入，`IAssemblyLifecycle` 接口保留为用户面契约。
/// </summary>
public class DefaultAssemblyLifecycle : IAssemblyLifecycle
{
    public string? OnResolving(AssemblyResolvingArgs args)
    {
        return args.RequestPath;
    }

    public void OnLoaded(AssemblyLoadArgs args) { }

    public void OnUnloading(AssemblyUnloadArgs args)
    {
        // 默认允许卸载（Cancel = false 不变）
    }

    public void OnUnloaded(AssemblyUnloadedArgs args) { }
}
