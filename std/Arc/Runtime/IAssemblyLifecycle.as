namespace Arc.Runtime;

// RFC 017 M3：程序集生命周期契约。
//
// ## 生命周期钩子
//
// 1. OnResolving → 2. OnLoaded → 3. OnUnloading → 4. OnUnloaded
//
// ## 事件参数封装
//
// 每个钩子使用专用 Args 类封装事件上下文（对齐 C# EventArgs 模式），方便后续扩展：
//   - AssemblyResolvingArgs：解析阶段（路径 + 请求方）
//   - AssemblyLoadArgs：加载完成（Assembly 实例 + 路径）
//   - AssemblyUnloadArgs：卸载前（Assembly 实例 + 可阻止）
//   - AssemblyUnloadedArgs：卸载后（名称）
//
// ## 依赖关系追踪
//
// 透过 Args 参数清晰表达：
//   - 谁加载的谁（RequestingAssembly）
//   - 谁依赖谁（_dependencyGraph）
//   - 生命周期阶段（loaded / unloading / unloaded）
//
// ## 编码模型
//
//   AssemblyLoadContext.Default
//     .SetLifecycle(new MyPluginLifecycle())
//     .Load("plugin.dll");
//
// ## 实现说明（RFC 017）
//
// std 侧对抽象类/接口字段的存储与分派无法正确 codegen（vtable 未实体化
// 导致 AV），`AssemblyLoadContext` 因此以具体类 `DefaultAssemblyLifecycle`
// 字段承载钩子；自定义生命周期**派生**该类、由入口包构造实例经
// `SetLifecycle` 注入（入口包构造的实例 vtable 完整）。本接口保留为
// 用户面契约（`DefaultAssemblyLifecycle : IAssemblyLifecycle`）。

// ============================================================
// 事件参数类
// ============================================================

/// <summary>
/// 程序集解析事件参数。
/// 当 Load 在文件系统找不到时触发，允许自定义发现逻辑。
/// </summary>
public class AssemblyResolvingArgs
{
    /// <summary>加载上下文。</summary>
    public AssemblyLoadContext Context { get; }

    /// <summary>请求的库路径（如 "plugins/sqlite3.dll"）。</summary>
    public string RequestPath { get; }

    /// <summary>触发加载的请求方（null = 顶层 Load 调用）。</summary>
    public Assembly? RequestingAssembly { get; }

    public AssemblyResolvingArgs(AssemblyLoadContext context, string path,
                                  Assembly? requesting) {
        this.Context = context;
        this.RequestPath = path;
        this.RequestingAssembly = requesting;
    }
}

/// <summary>
/// 程序集加载完成事件参数。
/// </summary>
public class AssemblyLoadArgs
{
    /// <summary>加载上下文。</summary>
    public AssemblyLoadContext Context { get; }

    /// <summary>加载后的实际路径。</summary>
    public string ResolvedPath { get; }

    /// <summary>已加载的 Assembly 实例。</summary>
    public Assembly LoadedAssembly { get; }

    /// <summary>触发加载的请求方（null = 顶层调用）。</summary>
    public Assembly? RequestingAssembly { get; }

    public AssemblyLoadArgs(AssemblyLoadContext context, string resolvedPath,
                             Assembly loaded, Assembly? requesting) {
        this.Context = context;
        this.ResolvedPath = resolvedPath;
        this.LoadedAssembly = loaded;
        this.RequestingAssembly = requesting;
    }
}

/// <summary>
/// 程序集卸载前事件参数。
/// UnloadingAssembly 是即将被卸载的 Assembly。
/// Cancel = true 可阻止卸载（安全网）。
/// </summary>
public class AssemblyUnloadArgs
{
    /// <summary>加载上下文。</summary>
    public AssemblyLoadContext Context { get; }

    /// <summary>即将被卸载的 Assembly。</summary>
    public Assembly UnloadingAssembly { get; }

    /// <summary>设为 true 可阻止本次卸载。</summary>
    public bool Cancel { get; set; }

    public AssemblyUnloadArgs(AssemblyLoadContext context, Assembly unloading) {
        this.Context = context;
        this.UnloadingAssembly = unloading;
        this.Cancel = false;
    }
}

/// <summary>
/// 程序集卸载完成事件参数。
/// Name 是已卸载程序集的名称（句柄已释放，仅名称可用）。
/// </summary>
public class AssemblyUnloadedArgs
{
    /// <summary>加载上下文。</summary>
    public AssemblyLoadContext Context { get; }

    /// <summary>已卸载的程序集名称。</summary>
    public string Name { get; }

    public AssemblyUnloadedArgs(AssemblyLoadContext context, string name) {
        this.Context = context;
        this.Name = name;
    }
}

// ============================================================
// 生命周期契约
// ============================================================

/// <summary>
/// 程序集生命周期契约（RFC 017 M3）。
///
/// `AssemblyLoadContext` 以具体类 `DefaultAssemblyLifecycle`（实现本接口）
/// 承载钩子并支持派生注入；本接口保留为用户面契约。
/// </summary>
public interface IAssemblyLifecycle
{
    // ---- 解析阶段 ----

    /// <summary>
    /// 依赖库解析——当 Load 在文件系统找不到时触发。
    /// 可通过 args.RequestPath 获取原始路径，args.RequestingAssembly 了解调用方。
    /// 返回解析后的实际路径；返回 null 表示无法解析。
    /// </summary>
    string? OnResolving(AssemblyResolvingArgs args);

    // ---- 加载完成 ----

    /// <summary>
    /// 程序集加载成功。
    /// args.LoadedAssembly 可立即使用；args.ResolvedPath 为实际加载路径。
    /// </summary>
    void OnLoaded(AssemblyLoadArgs args);

    // ---- 卸载前 ----

    /// <summary>
    /// 程序集卸载前通知。
    /// 设置 args.Cancel = true 可阻止卸载（安全网，默认 false）。
    /// </summary>
    void OnUnloading(AssemblyUnloadArgs args);

    // ---- 卸载完成 ----

    /// <summary>
    /// 程序集卸载完成。
    /// args.Name 为已卸载程序集的名称（句柄已释放，不可再使用 Assembly 实例）。
    /// </summary>
    void OnUnloaded(AssemblyUnloadedArgs args);
}
