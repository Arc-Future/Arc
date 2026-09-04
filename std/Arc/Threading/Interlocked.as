// Interlocked — 原子 int 操作最小面（RFC 009 §7.5）
//
// 诚实子集：Increment / Decrement / Exchange / CompareExchange（int）。
// Codegen 直降 LLVM atomicrmw / cmpxchg（seq_cst）；无 C 运行时包装。
// 后置：long/泛型/Add/Read/Write —— 禁止空 stub。

namespace Arc.Threading;

/// <summary>
/// 原子读写辅助——对齐 C# <c>System.Threading.Interlocked</c> 的 int 精华面。
/// </summary>
public class Interlocked {
    /// <summary>原子递增；返回递增后的新值。</summary>
    [Builtin(ABI = "Interlocked.Increment")]
    public static int Increment(ref int location) { return 0; }

    /// <summary>原子递减；返回递减后的新值。</summary>
    [Builtin(ABI = "Interlocked.Decrement")]
    public static int Decrement(ref int location) { return 0; }

    /// <summary>原子交换；返回交换前的旧值。</summary>
    [Builtin(ABI = "Interlocked.Exchange")]
    public static int Exchange(ref int location, int value) { return 0; }

    /// <summary>
    /// 原子比较并交换：若 <paramref name="location"/> 等于 <paramref name="comparand"/>，
    /// 则写入 <paramref name="value"/>。返回交换前的旧值。
    /// </summary>
    [Builtin(ABI = "Interlocked.CompareExchange")]
    public static int CompareExchange(ref int location, int value, int comparand) { return 0; }
}
