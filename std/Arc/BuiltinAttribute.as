// [Builtin] — 编译器内建方法标记（RFC XXX）
// 标记方法为编译器内建实现：跳过 body 类型检查，由 codegen 直接发射 ABI。
// 对标 C# 的 MethodImplOptions.InternalCall。
namespace Arc {

/// <summary>
/// 标记方法为编译器内建实现。
/// 有 [Builtin] 的方法体不被编译——typeck 跳过 body 检查，
/// codegen 根据 ABI 属性值发射对应的运行时 ABI 调用。
///
/// 无 [Builtin] 的方法在 facade 类中可作为真实 Arc 代码正常编译，
/// 与 C# 的 Parallel.ForEachAsync（真实 .NET 代码，非 InternalCall）行为对齐。
/// </summary>
[AttributeUsage(AttributeTargets.MethodOrProperty)]
public class BuiltinAttribute : Attribute {
    /// <summary>可选：显式指定 ABI 符号名（如 "rt_parallel_for"）。
    /// 为空时自动推导为 "ClassName.Method" 点号格式。</summary>
    public string ABI { get; set; }
}

} // namespace Arc
