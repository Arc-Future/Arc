// Arc.Diagnostics.RefCount — ARC 引用计数只读诊断（RFC 016 子项 M1）。
//
// 供确定性泄漏检测在 Arc 源码侧读取对象引用计数（对应运行时 `rt_arc_count`）。
// 纯诊断只读接口，非热路径：不参与任何生产逻辑，仅用于 faulted Task 异常
// 所有权等场景的引用计数归零断言（跨边界零泄漏主协议）。
//
// 单一惯用法：ARC 引用计数只在运行时 `rt_arc.c` 定义单一事实来源，本类型
// 仅暴露只读观察，绝无手工 inc/dec 配对。

namespace Arc.Diagnostics;

/// <summary>
/// ARC 引用计数只读观察器。
/// </summary>
public static class RefCount {
    /// <summary>读取对象当前 ARC 引用计数（非零即对象仍被引用）。</summary>
    [Builtin(ABI = "rt_arc_count")]
    public static int GetRefCount(object obj) { return 0; }
}