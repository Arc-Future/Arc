//! 热重载组合序列 L2 测试批（RFC 045 D8.1）。
//!
//! 内核 Reload（先装新后卸旧）与 ALC 二进制代数（RFC 017）正交组合的
//! ALC 侧语义验证：多代共存、Entry 烟测门禁（布局指纹）、正序卸载、
//! 门禁失败回滚。内核侧行为（服务面切换/失败回滚）已由
//! plugins_kernel_e2e 验收，本批验证 D8.1 编排序列在 ALC 侧的可执行性——
//! 编排逻辑位于宿主回调（D8.1：内核不感知二进制形态，组合点在回调内
//! 显式驱动 ALC），故用例以宿主编排器形态驱动，序列步骤与 D8.1 一一对应。
//!
//! 全部用例共享 `AssemblyLoadContext.Default` 单例，结束时必须把自身加载
//! 的插件卸载干净（防污染后续 case）。

#[cfg(feature = "full-rt")]
use arc_tests::{assert_compiles_and_runs_batch, compile_plugin_library, workspace_root};

#[cfg(feature = "full-rt")]
fn assert_all_passed(batch: &str, results: &[arc_tests::BatchRunResult]) {
    for r in results {
        assert!(
            r.passed,
            "{batch}: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}

/// 插件目录绝对路径。统一正斜杠：宿主源字符串无反斜杠转义问题。
#[cfg(feature = "full-rt")]
fn plugins_dir(batch: &str) -> String {
    let root = workspace_root().to_string_lossy().replace('\\', "/");
    format!("{root}/target/arc-tests/{batch}-plugins")
}

/// 换代 v1 插件：`HrPayload : HrBase`（override Describe）+ 无参 Entry。
/// 与 v2 同布局、独立路径——D8.1 第 1 步（新代独立路径，旧代卸载前被 OS
/// 锁定不可覆盖）。**HrBase 每插件内嵌副本**：插件编译为单文件直通
/// （deps 只写运行时 meta），跨插件源码级继承属 arc.toml path 依赖课题；
/// 迁移按 vtable slot 语义对齐（同名同签名 → slot 序一致），副本无碍。
#[cfg(feature = "full-rt")]
const PLUGIN_HR_V1_BODY: &str = r#"namespace PluginHr;

public class HrBase
{
    public string Tag;

    public virtual string Describe() { return Tag + ":base"; }
}

public class HrPayload : HrBase
{
    public override string Describe() { return Tag + ":hr-v1-method"; }
}

public HrPayload Entry()
{
    HrPayload payload = new HrPayload();
    payload.Tag = "hr-v1";
    return payload;
}

public static class Probe
{
    public static HrBase New() { return new HrBase(); }
}
"#;

/// 换代 v2 插件：与 v1 同名同布局同 vtable 形状（Describe():string 同签名、
/// 实现各异）+ 独立路径——多重判定全等 → 透明迁移放行；Describe 实现
/// （":hr-v2-method"）为迁移后虚分派验证的观察点。HrBase 内嵌副本同 v1
/// （含 Probe 哨兵）。
#[cfg(feature = "full-rt")]
const PLUGIN_HR_V2_BODY: &str = r#"namespace PluginHr;

public class HrBase
{
    public string Tag;

    public virtual string Describe() { return Tag + ":base"; }
}

public class HrPayload : HrBase
{
    public override string Describe() { return Tag + ":hr-v2-method"; }
}

public HrPayload Entry()
{
    HrPayload payload = new HrPayload();
    payload.Tag = "hr-v2";
    return payload;
}

public static class Probe
{
    public static HrBase New() { return new HrBase(); }
}
"#;

/// 门禁负向插件：与宿主/旧代同名 `HrPayload` 但**多一个字段**（同名异构）
/// ——Entry 符号的布局指纹段必异 → 烟测显式 `EntryPointNotFoundException`
/// → 编排器回滚（旧代保持运行）。
#[cfg(feature = "full-rt")]
const PLUGIN_HR_V2_MUTANT_BODY: &str = r#"namespace PluginHr;

public class HrPayload
{
    public string Tag;
    public int Extra;
}

public HrPayload Entry()
{
    HrPayload payload = new HrPayload();
    payload.Tag = "hr-v2-mutant";
    payload.Extra = 7;
    return payload;
}
"#;

// ============================================================
// 用例 1：正序换代全链路（D8.1 序列步骤 1-5）
// ============================================================
//
// 载 v1 → Entry → Tag 断言（旧代服务）→ 载 v2（多代共存）→ Entry 烟测
// （指纹门禁通过）→ 切面完成 → 卸 v1（旧代退役，前置条件：宿主引用已
// 置 null）→ v1 退役断言 → v2 持续服务（再 Entry）→ 卸 v2 → 清理断言。

#[cfg(feature = "full-rt")]
fn swap_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

class HrBase
{{
    public string Tag;

    public virtual string Describe() {{ return Tag + ":host"; }}
}}

class HrPayload : HrBase
{{
    public override string Describe() {{ return Tag + ":host-p"; }}
}}

void Main()
{{
    try
    {{
    string pluginsDir = "{dir}";
    string pathV1 = pluginsDir + "/plugin_hr_v1.dll";
    string pathV2 = pluginsDir + "/plugin_hr_v2.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;

        Assembly v1 = alc.Load(pathV1);
        if (v1 == null) {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:load-v1"); return; }}
    HrPayload p1 = v1.Entry<HrPayload>();
        if (p1 == null) {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:v1-null"); return; }}
    if (p1.Tag != "hr-v1") {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:v1-tag=" + p1.Tag); return; }}
    p1 = null;

        Assembly v2 = alc.Load(pathV2);
        if (v2 == null) {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:load-v2"); return; }}
    HrPayload p2 = v2.Entry<HrPayload>();
        if (p2 == null) {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:v2-smoke-null"); return; }}
    if (p2.Tag != "hr-v2") {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:v2-smoke-tag=" + p2.Tag); return; }}
    p2 = null;

    alc.Unload(v1);
    if (alc.GetLoadedAssembly(pathV1) != null) {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:v1-retired"); return; }}

    HrPayload p3 = v2.Entry<HrPayload>();
    if (p3 == null) {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:v2-serve-null"); return; }}
    if (p3.Tag != "hr-v2") {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:v2-serve-tag=" + p3.Tag); return; }}
    p3 = null;

    alc.Unload(v2);
    if (alc.GetLoadedAssembly(pathV2) != null) {{ Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:cleanup-v2"); return; }}
    Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:PASS");
    }}
    catch (Exception e)
    {{
        Console.WriteLine("ARC_CASE:hr_seamless_generation_swap:FAIL:unhandled=" + e.Message);
    }}
}}
"#
    )
}

// ============================================================
// 用例 2：指纹门禁回滚（D8.1 序列步骤 3 失败分支）
// ============================================================
//
// 载 v1（正常服务）→ 载同名异构 v2mutant（宿主按 1 字段声明调 2 字段
// 插件）→ Entry 烟测显式 `EntryPointNotFoundException` → 回滚：不切面、
// 继续用 v1（旧代持续服务断言）→ 清理失败代 v2mutant（无依赖方 → 护栏
// 放行）→ 卸 v1 → 清理断言。红基线：指纹未生效时符号按类型 id 匹配 →
// 宿主按 1 字段布局读 2 字段对象（静默 UB），由 FAIL 标签转显式。

#[cfg(feature = "full-rt")]
fn gate_rollback_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    string pluginsDir = "{dir}";
    string pathV1 = pluginsDir + "/plugin_hr_v1.dll";
    string pathMutant = pluginsDir + "/plugin_hr_v2_mutant.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;

    Assembly v1 = alc.Load(pathV1);
    if (v1 == null) {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:load-v1"); return; }}
    HrPayload p1 = v1.Entry<HrPayload>();
    if (p1 == null) {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:v1-null"); return; }}
    if (p1.Tag != "hr-v1") {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:v1-tag=" + p1.Tag); return; }}
    p1 = null;

    Assembly mutant = alc.Load(pathMutant);
    if (mutant == null) {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:load-mutant"); return; }}
    string gateMsg = "";
    try
    {{
        HrPayload pBad = mutant.Entry<HrPayload>();
        Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:silent-match");
        return;
    }}
    catch (EntryPointNotFoundException e)
    {{
        gateMsg = e.Message;
    }}
    if (gateMsg.IndexOf("entry point not found") < 0) {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:msg=" + gateMsg); return; }}

    HrPayload p2 = v1.Entry<HrPayload>();
    if (p2 == null) {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:v1-serve-null"); return; }}
    if (p2.Tag != "hr-v1") {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:v1-serve-tag=" + p2.Tag); return; }}
    p2 = null;

    alc.Unload(mutant);
    if (alc.GetLoadedAssembly(pathMutant) != null) {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:cleanup-mutant"); return; }}
    alc.Unload(v1);
    if (alc.GetLoadedAssembly(pathV1) != null) {{ Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:FAIL:cleanup-v1"); return; }}
    Console.WriteLine("ARC_CASE:hr_fingerprint_gate_rollback:PASS");
}}
"#
    )
}

// ============================================================
// 用例 3：编排 API 换代（D8.1 序列的 AssemblyHotReload 命名化）
// ============================================================
//
// IsLayoutCompatible 判定通过（同布局两代）→ 切面 → RetireGeneration 退役
// 旧代 → 新代持续服务。烟测仍由宿主直接调 Entry（D8.1：编排器无法泛化
// 泛型实参）。

#[cfg(feature = "full-rt")]
fn orchestrated_swap_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    string pluginsDir = "{dir}";
    string pathV1 = pluginsDir + "/plugin_hr_v1.dll";
    string pathV2 = pluginsDir + "/plugin_hr_v2.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;

    Assembly v1 = alc.Load(pathV1);
    if (v1 == null) {{ Console.WriteLine("ARC_CASE:hr_orchestrated_swap:FAIL:load-v1"); return; }}
    HrPayload p1 = v1.Entry<HrPayload>();
    if (p1 == null || p1.Tag != "hr-v1") {{ Console.WriteLine("ARC_CASE:hr_orchestrated_swap:FAIL:v1-smoke"); return; }}
    p1 = null;

    Assembly v2 = alc.Load(pathV2);
    if (v2 == null) {{ Console.WriteLine("ARC_CASE:hr_orchestrated_swap:FAIL:load-v2"); return; }}
    HrPayload p2 = v2.Entry<HrPayload>();
    if (p2 == null || p2.Tag != "hr-v2") {{ Console.WriteLine("ARC_CASE:hr_orchestrated_swap:FAIL:v2-smoke"); return; }}
    p2 = null;

    if (!AssemblyHotReload.IsLayoutCompatible(v1, v2, "HrPayload")) {{ Console.WriteLine("ARC_CASE:hr_orchestrated_swap:FAIL:compatible-same-layout-rejected old=" + v1.GetLayoutSignature("HrPayload") + " new=" + v2.GetLayoutSignature("HrPayload")); return; }}

    AssemblyHotReload.RetireGeneration(alc, v1);
    if (alc.GetLoadedAssembly(pathV1) != null) {{ Console.WriteLine("ARC_CASE:hr_orchestrated_swap:FAIL:v1-retired"); return; }}

    HrPayload p3 = v2.Entry<HrPayload>();
    if (p3 == null || p3.Tag != "hr-v2") {{ Console.WriteLine("ARC_CASE:hr_orchestrated_swap:FAIL:v2-serve"); return; }}
    p3 = null;

    AssemblyHotReload.RetireGeneration(alc, v2);
    if (alc.GetLoadedAssembly(pathV2) != null) {{ Console.WriteLine("ARC_CASE:hr_orchestrated_swap:FAIL:cleanup-v2"); return; }}
    Console.WriteLine("ARC_CASE:hr_orchestrated_swap:PASS");
}}
"#
    )
}

// ============================================================
// 用例 4：状态搬运 MVP（状态迁移 L1 判定 + L2 应用层搬运）
// ============================================================
//
// 旧代状态对象（宿主写入用户数据）→ IsLayoutCompatible 判定：同布局
// v2 = true（可搬运）、同名异构 mutant = false（保守拒绝）→ 失败代
// AbortGeneration 清理 → 判定通过后载 v2、把旧状态写入新代对象 → 退役
// v1 → 新代对象携带迁移状态持续服务。L3 透明对象图迁移为 RFC 议题
// （rt 层对象重绑，未实施）——本用例锚定 L1/L2 的可实施水位。

#[cfg(feature = "full-rt")]
fn state_handover_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    string pluginsDir = "{dir}";
    string pathV1 = pluginsDir + "/plugin_hr_v1.dll";
    string pathV2 = pluginsDir + "/plugin_hr_v2.dll";
    string pathMutant = pluginsDir + "/plugin_hr_v2_mutant.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;

    Assembly v1 = alc.Load(pathV1);
    if (v1 == null) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:load-v1"); return; }}
    HrPayload state = v1.Entry<HrPayload>();
    if (state == null) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:v1-null"); return; }}
    state.Tag = "user-data-42";

    Assembly mutant = alc.Load(pathMutant);
    if (mutant == null) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:load-mutant"); return; }}
    if (AssemblyHotReload.IsLayoutCompatible(v1, mutant, "HrPayload")) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:mutant-judged-compatible"); return; }}
    AssemblyHotReload.AbortGeneration(alc, mutant);
    if (alc.GetLoadedAssembly(pathMutant) != null) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:mutant-leftover"); return; }}

    Assembly v2 = alc.Load(pathV2);
    if (v2 == null) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:load-v2"); return; }}
    if (!AssemblyHotReload.IsLayoutCompatible(v1, v2, "HrPayload")) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:same-layout-rejected"); return; }}

    HrPayload carried = v2.Entry<HrPayload>();
    if (carried == null) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:v2-null"); return; }}
    carried.Tag = state.Tag;
    if (carried.Tag != "user-data-42") {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:handover=" + carried.Tag); return; }}

    state = null;
    AssemblyHotReload.RetireGeneration(alc, v1);
    if (alc.GetLoadedAssembly(pathV1) != null) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:v1-retired"); return; }}
    if (carried.Tag != "user-data-42") {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:post-retire-state=" + carried.Tag); return; }}

    carried = null;
    AssemblyHotReload.RetireGeneration(alc, v2);
    if (alc.GetLoadedAssembly(pathV2) != null) {{ Console.WriteLine("ARC_CASE:hr_state_handover:FAIL:cleanup-v2"); return; }}
    Console.WriteLine("ARC_CASE:hr_state_handover:PASS");
}}
"#
    )
}

// ============================================================
// 用例 5：L3 透明对象图迁移（RFC 047 实施验收）
// ============================================================
//
// 状态对象登记为模块根（RegisterModuleRoot——长驻插件状态的正确编排）→
// MigrateInstances 根 DFS 重绑（双重判定放行）→ 字段内存保持（Tag 原值）
// → 根转移（旧代解绑、新代接管）→ Retire 旧代（vtable 已重绑 → 卸载后
// 对象存活）→ **p1 = null 的 dec 走新 vtable——未迁移时此处必 AV（旧
// vtable 随卸载解除映射），PASS 即重绑的行为学铁证**。

#[cfg(feature = "full-rt")]
fn transparent_migration_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    try
    {{
    string pluginsDir = "{dir}";
    string pathV1 = pluginsDir + "/plugin_hr_v1.dll";
    string pathV2 = pluginsDir + "/plugin_hr_v2.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;

    Assembly v1 = alc.Load(pathV1);
    if (v1 == null) {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:load-v1"); return; }}
    HrPayload state = v1.Entry<HrPayload>();
    if (state == null) {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:v1-null"); return; }}
    state.Tag = "user-data-42";
    if (!alc.RegisterModuleRoot(v1, state)) {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:root-register"); return; }}

    Assembly v2 = alc.Load(pathV2);
    if (v2 == null) {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:load-v2"); return; }}
    int migrated = AssemblyHotReload.MigrateInstances(v1, v2);
    if (migrated < 1) {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:migrate-count=" + migrated); return; }}

    if (state.Tag != "user-data-42") {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:state-lost=" + state.Tag); return; }}
    alc.UnregisterModuleRoot(v1, state);
    alc.RegisterModuleRoot(v2, state);
    AssemblyHotReload.RetireGeneration(alc, v1);
    if (alc.GetLoadedAssembly(pathV1) != null) {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:v1-retired"); return; }}
    if (state.Tag != "user-data-42") {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:post-retire-state=" + state.Tag); return; }}

    // 收尾顺序（root 登记契约：登记即移交释放责任，release_roots 卸载时 dec）：
    // 先解绑根（收回释放责任）→ 宿主置 null（dec → 对象释放，走新 vtable）
    // → Retire v2（release_roots 时 state 已不在根表，无双重释放）。
    alc.UnregisterModuleRoot(v2, state);
    state = null;
    AssemblyHotReload.RetireGeneration(alc, v2);
    if (alc.GetLoadedAssembly(pathV2) != null) {{ Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:cleanup-v2"); return; }}
    Console.WriteLine("ARC_CASE:hr_transparent_migration:PASS");
    }}
    catch (Exception e)
    {{
        Console.WriteLine("ARC_CASE:hr_transparent_migration:FAIL:unhandled=" + e.Message);
    }}
}}
"#
    )
}

// ============================================================
// 用例 6：迁移后虚分派命中新代实现（RFC 047 验收 §7.1）
// ============================================================
//
// 状态字段 Tag 上移基类 HrBase（单变量 `HrBase state = v1.Entry<...>()`
// 向上转型接收——避免双引用的计数歧义）。三段铁证：
// ① 迁移前 `state.Describe()` = Tag + ":hr-v1-method"（旧代虚分派）；
// ② MigrateInstances 后同引用同调用 = Tag + ":hr-v2-method"——**字段状态
//    保持（Tag 原值）与方法分派切换（v2 实现）同帧实证**；
// ③ Retire v1（dlclose）后再调用仍 = ":hr-v2-method"（旧 vtable 已解除
//    映射而分派继续有效——重绑的存续性铁证）。宿主自己的 HrPayload
//    Describe（":host-p"）全程不被命中——分派走的是对象头的 vtable，
//    静态类型不参与。

#[cfg(feature = "full-rt")]
fn virtual_dispatch_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    string pluginsDir = "{dir}";
    string pathV1 = pluginsDir + "/plugin_hr_v1.dll";
    string pathV2 = pluginsDir + "/plugin_hr_v2.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;

    Assembly v1 = alc.Load(pathV1);
    if (v1 == null) {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:load-v1"); return; }}
    HrBase state = v1.Entry<HrPayload>();
    if (state == null) {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:v1-null"); return; }}
    if (state.Describe() != "hr-v1:hr-v1-method") {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:pre-dispatch=" + state.Describe()); return; }}
    state.Tag = "user-data-42";
    if (!alc.RegisterModuleRoot(v1, state)) {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:root-register"); return; }}

    Assembly v2 = alc.Load(pathV2);
    if (v2 == null) {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:load-v2"); return; }}
    int migrated = AssemblyHotReload.MigrateInstances(v1, v2);
    if (migrated < 1) {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:migrate-count=" + migrated); return; }}

    if (state.Describe() != "user-data-42:hr-v2-method") {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:post-dispatch=" + state.Describe()); return; }}
    alc.UnregisterModuleRoot(v1, state);
    alc.RegisterModuleRoot(v2, state);
    AssemblyHotReload.RetireGeneration(alc, v1);
    if (alc.GetLoadedAssembly(pathV1) != null) {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:v1-retired"); return; }}
    if (state.Describe() != "user-data-42:hr-v2-method") {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:post-retire-dispatch=" + state.Describe()); return; }}

    alc.UnregisterModuleRoot(v2, state);
    state = null;
    AssemblyHotReload.RetireGeneration(alc, v2);
    if (alc.GetLoadedAssembly(pathV2) != null) {{ Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:FAIL:cleanup-v2"); return; }}
    Console.WriteLine("ARC_CASE:hr_virtual_dispatch_after_migration:PASS");
}}
"#
    )
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_hot_reload_batch() {
    let batch = "hot_reload_batch";
    // 插件供给先于宿主批：宿主运行时探针按文件命中，编译期无插件感知。
    compile_plugin_library(batch, "plugin_hr_v1", PLUGIN_HR_V1_BODY, &[]);
    compile_plugin_library(batch, "plugin_hr_v2", PLUGIN_HR_V2_BODY, &[]);
    compile_plugin_library(batch, "plugin_hr_v2_mutant", PLUGIN_HR_V2_MUTANT_BODY, &[]);

    let dir = plugins_dir(batch);
    let results = assert_compiles_and_runs_batch(
        batch,
        &[
            ("hr_seamless_generation_swap", &swap_case(&dir)),
            ("hr_fingerprint_gate_rollback", &gate_rollback_case(&dir)),
            ("hr_orchestrated_swap", &orchestrated_swap_case(&dir)),
            ("hr_state_handover", &state_handover_case(&dir)),
            ("hr_transparent_migration", &transparent_migration_case(&dir)),
            ("hr_virtual_dispatch_after_migration", &virtual_dispatch_case(&dir)),
        ],
    );
    assert_all_passed(batch, &results);
}
