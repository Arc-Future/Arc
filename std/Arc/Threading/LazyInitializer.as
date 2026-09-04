// LazyInitializer — one-shot field initialization helper (C# System.Threading essence).
//
// Minimal honest surface: EnsureInitialized(ref T, ref bool, Lock, Func<T>).
// Uses Lock (not arbitrary object) — same sync model as Monitor / lock statement.
// Maturity: Stable 最小面（随 Lazy<T>）— EnsureInitialized 可证伪；无 class-null 快路径双轨。
// Interlocked int 面已另立（RFC 009 §7.5）。

namespace Arc.Threading;

using Arc;

/// <summary>
/// Static helpers for lazy field initialization under an explicit <see cref="Lock"/>.
/// Aligns with C# <c>LazyInitializer.EnsureInitialized(ref T, ref bool, ref object, Func&lt;T&gt;)</c>
/// with Arc's dedicated <c>Lock</c> type (RFC 009 §7.2).
/// </summary>
/// <remarks>
/// 成熟度：Stable 最小面（与 <c>Lazy&lt;T&gt;</c> 同口径）。Lock 双检可证伪；无 class-null 快路径双轨。
/// </remarks>
public static class LazyInitializer {
    /// <summary>
    /// If <paramref name="initialized"/> is false, runs <paramref name="factory"/> under
    /// <paramref name="syncLock"/> once and stores the result into <paramref name="target"/>.
    /// </summary>
    public static T EnsureInitialized<T>(
        ref T target,
        ref bool initialized,
        Lock syncLock,
        Func<T> factory
    ) {
        if (initialized) {
            return target;
        }
        lock (syncLock) {
            if (!initialized) {
                Func<T> f = factory;
                target = f();
                initialized = true;
            }
        }
        return target;
    }
}
