namespace Arc.Runtime;

// ============================================================
// AssemblyHotReload —— 换代编排（RFC 045 D8.1 序列的 ALC 侧命名化）
// ============================================================

/// <summary>
/// 二进制插件热重载的编排原语（RFC 045 D8.1 组合契约）。
///
/// 六步换代序列中的 ALC 侧步骤在此命名化：烟测门禁由调用方对新代直接
/// 调用 <c>Assembly.Entry&lt;T&gt;()</c> 完成（宿主知道泛型实参，编排器无法
/// 泛化），本类承载判定与退役/清理语义。内核 Reload 负责服务面切换与
/// 失败回滚（先装新、成功后再卸旧），ALC 负责二进制代数生命周期——
/// 组合点在插件安装/卸载回调内显式驱动，本类不感知服务拓扑。
/// </summary>
public static class AssemblyHotReload
{
    /// <summary>
    /// 状态迁移 L1 兼容性判定（D8.1 状态迁移分层）：两代程序集的同名类型
    /// 布局指纹一致 → 结构兼容（字段内存可搬运）；任一未物化（0）或指纹
    /// 异 → 不兼容（<b>保守拒绝</b>——未知 ≠ 兼容，禁静默错配）。
    ///
    /// 指纹为编译期 <c>entry_layout_signature</c>（FNV-1a-64 布局传递闭包，
    /// 含嵌套字段类型的深层变化）的物化值，随 <c>__arc_package_meta</c>
    /// 第 5 字段分发。
    /// </summary>
    public static bool IsLayoutCompatible(Assembly oldGen, Assembly newGen, string typeName)
    {
        if (oldGen == null || newGen == null) { return false; }
        if (typeName == null || typeName.Length == 0) { return false; }
        long oldSig = oldGen.GetLayoutSignature(typeName);
        if (oldSig == 0) { return false; }
        long newSig = newGen.GetLayoutSignature(typeName);
        if (newSig == 0) { return false; }
        return oldSig == newSig;
    }

    /// <summary>
    /// D8.1 序列步骤 5：退役旧代。调用前置 = 服务面已无旧代对象引用
    /// （内核效果撤销 + InjectReactive 断开 + 宿主跨界引用置 null）——
    /// 本方法复用 <c>AssemblyLoadContext.Unload</c> 的三道护栏（ledger /
    /// in-flight / 被依赖感知 E_UNLOAD_DEPENDED），前置不满足时抛异常
    /// 而非静默卸载。卸载后旧代句柄保留供悬垂访问检测。
    /// </summary>
    public static void RetireGeneration(AssemblyLoadContext alc, Assembly oldGen)
    {
        if (alc == null) { throw new InvalidOperationException("AssemblyHotReload: alc is null."); }
        if (oldGen == null) { return; }
        alc.Unload(oldGen);
    }

    /// <summary>
    /// D8.1 门禁失败路径：清理未晋级的失败代（烟测不通过 / apply 异常）。
    /// 幂等——失败代可能从未被加载或已被并发清理。失败代无在载依赖方
    /// （尚未投入使用），卸载顺序护栏恒放行；前置条件不满足（调用方仍持
    /// 失败代对象引用）时由 Unload 抛异常报告，不静默。
    /// </summary>
    public static void AbortGeneration(AssemblyLoadContext alc, Assembly failedGen)
    {
        if (alc == null) { throw new InvalidOperationException("AssemblyHotReload: alc is null."); }
        if (failedGen == null) { return; }
        if (failedGen.IsDisposed) { return; }
        alc.Unload(failedGen);
    }

    /// <summary>
    /// RFC 047 L3 透明对象图迁移：将 oldGen 根可达闭包内的实例原地重绑到
    /// newGen 同构类型（vtable 头重绑——字段内存/对象地址/引用计数全部
    /// 保持，使用者无感知）。前置 = newGen 已通过 Entry 烟测门禁、旧代处于
    /// 可卸载前置态（宿主引用已清理）。
    ///
    /// 返回迁移对象数。rt 层逐类型执行**双重判定**（字段布局指纹 + vtable
    /// 形状指纹全等）；任一类型不兼容 → 整体拒绝（抛 InvalidOperationException，
    /// 旧代保持原样——回滚零成本），编排器降级 L2 搬运或拒绝换代。
    /// </summary>
    public static int MigrateInstances(Assembly oldGen, Assembly newGen)
    {
        if (oldGen == null || newGen == null) {
            throw new InvalidOperationException("AssemblyHotReload.MigrateInstances: generation is null.");
        }
        int rc = rt_library.rt_library_migrate_instances(oldGen.Generation, newGen.Generation);
        if (rc == -3) {
            throw new InvalidOperationException(
                "RFC 047: transparent migration refused — vtable-shape or layout " +
                "incompatible type(s) present between the two generations. " +
                "Fall back to explicit state handover (L2) or abort the swap.");
        }
        if (rc < 0) {
            throw new InvalidOperationException(
                "RFC 047: migration failed rc=" + rc +
                " (-1 invalid generation, -2 vtable registry missing/malformed).");
        }
        return rc;
    }
}
