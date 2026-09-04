// Weak<T> — weak reference wrapper (RFC 005 §2.2).
//
// Single idiom (RFC 002): C# WeakReference<T> essence — does not strong-ref
// the target; TryGet atomically upgrades to a strong ref or returns null.
//
// Maturity: Stable 最小面 — construct + TryGet + drop semantics (weak_ref_e2e).
// No unsafe; no side table; no raw pointer exposure (RFC 005 compliant).
//
// 热卸载边界语义（RFC 017 §2.6）：模块外→模块内对象的引用推荐 Weak<T>
// （不阻止卸载）；宿主侧弱登记表（AssemblyLoadContext.RegisterWeakReference）
// 在模块卸载时中和已登记槽位 → 卸载后 TryGet() 确定性返回 null。详见本类
// remarks 与 rt_abi.h weak 注释块。
//
// ABI: Weak<T> is a real Arc class (has ArcHeader, gets inc'd/dec'd normally
// when stored in fields/locals). The _target field at offset 16 holds an
// opaque RtWeak* slot allocated by rt_arc_weak_create — codegen emits the
// store directly via the ctor stub, bypassing FieldSet ARC maintenance (the
// slot is not an ArcHeader object; rt_arc_inc would corrupt its first field).
//
// Drop sequence (arc_drop.rs Weak_* special case):
//   1. check rt_arc_count(weakobj) == 1 — this drop is the final reference
//   2. only then: rt_arc_weak_destroy(slot) — decs target.weakcount, frees slot,
//      and frees target header if both refcount and weakcount hit 0
//   3. rt_arc_dec(weakobj) — normal ARC dec on the Weak<T> object itself
//
// The slot is owned by the Weak<T> object, not by individual references:
// container slots (List<Weak<T>> elements, fields) and temporary references
// (stack locals / by-value copies) share the same object, so destroying the
// slot on every reference release double-frees it. The refcount gate destroys
// it exactly once (on the last release). Residual: when the last reference is
// released via a runtime container path (rt_list_arc_dec_ref → plain
// rt_arc_dec), the slot/target header leak (no AV/DF); closing that requires
// a vtable finalizer or rt_arc_weak_dec (rt_arc.c,另行排期).
//
// Constraint: T must be a reference type (class), enforced declaratively by
// `where T : class` on the type declaration — struct / enum / variant args
// are rejected at typeck instantiation time (check_constraints, C# semantics).

namespace Arc;

/// <summary>
/// Weak reference wrapper. Does not prevent the target from being reclaimed.
/// <c>TryGet</c> returns the target as a strong reference, or <c>null</c> if
/// the target has already been collected.
/// </summary>
/// <remarks>
/// 成熟度：Stable 最小面（构造 + TryGet + 析构语义；weak_ref_e2e 可证伪）。
/// 实现使用 RFC 005 §2.1 内联 <c>weakcount</c>（ArcHeader 16B 不变）；
/// <b>无</b> <c>unsafe</c>；<b>无</b> 裸指针暴露（RFC 005 一致）；
/// <b>无</b> Swift side table（复杂；不采纳）。
/// <para>
/// 语义约束：T 必须为引用类型（<c>where T : class</c> 声明强制）。
/// 值类型（struct/enum/variant）无共享语义。
/// </para>
/// <para>
/// <b>热卸载边界语义</b>（RFC 017 §2.6）：模块外对模块内对象的引用推荐
/// <c>Weak&lt;T&gt;</c>——不强引用 → ledger 归零 → 模块可卸载；模块内对象
/// 回收后 <c>TryGet()</c> 返回 null。模块边界弱引用经
/// <c>AssemblyLoadContext.RegisterWeakReference(asm, weak)</c> 登记进宿主侧
/// 弱登记表（本类内部 <c>GetWeakSlot()</c> 提供不透明槽位，用户面不暴露）；
/// 模块卸载时运行时中和已登记槽位（target 置空，观察 tombstone 头语义）→
/// 卸载后 <c>TryGet()</c> 确定性返回 null（非悬垂、禁复活）。弱引用
/// <b>不</b> 阻止卸载。
/// </para>
/// </remarks>
public class Weak<T> where T : class {
    /// <summary>
    /// Opaque RtWeak* slot stored at offset 16. Declared as <c>int</c> for
    /// typeck purposes; the codegen ctor stub stores the actual <c>ptr</c>
    /// returned by <c>rt_arc_weak_create</c> directly (bypassing FieldSet ARC).
    /// </summary>
    private int _target;

    /// <summary>
    /// Create a weak reference to <paramref name="target"/>. Does not
    /// strong-ref the target; increments the target's weakcount instead.
    /// </summary>
    [Builtin(ABI = "rt_arc_weak_create")]
    public Weak(T target) {
        _target = 0;
    }

    /// <summary>
    /// Try to obtain a strong reference to the target. Returns the target
    /// (with refcount incremented) if still alive, or <c>null</c> if the
    /// target has already been collected.
    /// </summary>
    [Builtin(ABI = "rt_arc_weak_try_get")]
    public T TryGet() {
        return null;
    }

    /// <summary>
    /// 返回本弱引用的不透明 RtWeak* 槽位（内部；供
    /// <c>AssemblyLoadContext.RegisterWeakReference</c> 边界登记）。槽位不
    /// 透明、用户面不暴露（RFC 005）；codegen stub 直接读取 offset 16。
    /// </summary>
    internal NativePtr GetWeakSlot() {
        return null;
    }
}
