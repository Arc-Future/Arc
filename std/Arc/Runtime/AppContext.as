// AppContext —— 应用上下文（RFC 017「应用上下文 AppContext」节）。
//
// 对标 C# System.AppContext 常用子集：应用基目录、功能开关、数据槽。
// 语义、解析链与偏离说明见 RFC 017；本文件为实现面。
namespace Arc.Runtime;

using Arc.Collections.Concurrent;
using Arc.IO;

/// <summary>
/// 应用级上下文对象——对齐 C# <c>System.AppContext</c> 常用子集。
///
/// 提供：
/// - <b>BaseDirectory</b>：应用基目录（解析链：当前执行程序集目录 →
///   <c>ARC_BASE_DIR</c> 环境变量 → 当前工作目录；首触惰性缓存）。
/// - <b>功能开关</b>：<see cref="SetSwitch"/> / <see cref="TryGetSwitch"/>
///   ——应用内特性开关（对齐 .NET 兼容性开关机制）。
/// - <b>数据槽</b>：<see cref="SetData"/> / <see cref="GetData"/>
///   ——应用级键值数据（值为 class 实例或 null）。
///
/// 与 .NET 的偏离（RFC 017）：
/// - 不提供 <c>TargetFrameworkName</c>——Arc 无 .NET 框架概念，无诚实值可暴露。
/// - Arc 编译器当前限制 <c>static class</c> 不支持静态字段，故以普通类 +
///   private 构造承载静态成员（对齐 <c>DependencyPropertyRegistry</c> 先例）；
///   用户面仍为纯静态访问。
/// - 开关/数据以 <c>ConcurrentDictionary</c> 承载（线程安全）；null/空名防御式
///   忽略而非抛异常。
/// - <c>BaseDirectory</c> 无尾随目录分隔符（<c>Path.Combine</c> 智能拼接）。
/// </summary>
public class AppContext
{
    /// <summary>功能开关表（线程安全；per-stripe 锁）。</summary>
    private static readonly ConcurrentDictionary<string, bool> _switches =
        new ConcurrentDictionary<string, bool>();

    /// <summary>应用数据槽（线程安全；class 值经 codegen retain，语义同 Dictionary）。</summary>
    private static readonly ConcurrentDictionary<string, object> _data =
        new ConcurrentDictionary<string, object>();

    /// <summary>
    /// 基目录——首触惰性缓存（RFC 006 A3 S6a：静态字段首触构造一次、线程安全）。
    /// </summary>
    private static readonly string _baseDirectory = AppContext.ComputeBaseDirectory();

    /// <summary>防止实例化——所有成员均为 static。</summary>
    private AppContext()
    {
    }

    /// <summary>
    /// 应用基目录。
    ///
    /// 解析链（首个非空即取）：
    /// 1. 当前执行程序集所在目录（<see cref="Assembly.GetExecutingAssembly"/> →
    ///    <see cref="Path.GetDirectoryName"/>）；
    /// 2. <c>ARC_BASE_DIR</c> 环境变量（Arc 特有扩展）；
    /// 3. 当前工作目录（<see cref="Environment.GetCurrentDirectory"/>）。
    ///
    /// 首次访问时确定并缓存；无尾随目录分隔符。
    /// </summary>
    public static string BaseDirectory
    {
        get { return _baseDirectory; }
    }

    /// <summary>设置或覆盖功能开关。</summary>
    /// <param name="name">开关名；null/空串忽略。</param>
    /// <param name="isEnabled">开关值。</param>
    public static void SetSwitch(string name, bool isEnabled)
    {
        if (name == null || name.Length == 0) { return; }
        _switches[name] = isEnabled;
    }

    /// <summary>读取功能开关。</summary>
    /// <param name="name">开关名；null/空串视为未定义。</param>
    /// <param name="isEnabled">输出开关值；未定义输出 false。</param>
    /// <returns>开关已定义返回 true；否则 false。</returns>
    public static bool TryGetSwitch(string name, out bool isEnabled)
    {
        isEnabled = false;
        if (name == null || name.Length == 0) { return false; }
        // out 形参转发给 stub 方法（TryGetValue 经 out 指针写入）：值在用户方法
        // 边界传播由 codegen byref 转发保证（CD-7 邻域修复）。
        return _switches.TryGetValue(name, out isEnabled);
    }

    /// <summary>设置应用数据槽。</summary>
    /// <param name="name">槽名；null/空串忽略。</param>
    /// <param name="value">槽值（class 实例或 null；Arc string 为纯 C-string，
    /// 非 object 子类型，方法实参路径未接线装箱——禁止直接传 string，见 RFC 017）。</param>
    public static void SetData(string name, object? value)
    {
        if (name == null || name.Length == 0) { return; }
        _data[name] = value;
    }

    /// <summary>读取应用数据槽。</summary>
    /// <param name="name">槽名；null/空串返回 null。</param>
    /// <returns>槽值（class 实例）；未定义或值为 null 均返回 null。</returns>
    public static object? GetData(string name)
    {
        if (name == null || name.Length == 0) { return null; }
        return _data.GetValueOrDefault(name);
    }

    /// <summary>解析基目录（静态字段初始器调用一次）。</summary>
    private static string ComputeBaseDirectory()
    {
        Assembly? asm = Assembly.GetExecutingAssembly();
        if (asm != null)
        {
            string dir = Path.GetDirectoryName(asm.Name);
            if (dir != null && dir.Length > 0) { return dir; }
        }
        string env = Environment.GetEnvironmentVariable("ARC_BASE_DIR");
        if (env != null && env.Length > 0) { return env; }
        return Environment.GetCurrentDirectory();
    }
}
