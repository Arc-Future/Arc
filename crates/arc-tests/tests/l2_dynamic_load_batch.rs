//! U5 动态加载 L2 测试批（UX 迭代评审 §2.5）。
//!
//! 热卸载闭环（RFC 017）的七类对抗面：并发竞态、同路径重载代数切换、
//! 卸载弱槽中和、OnUnloading Cancel 回退、传递依赖递归、hanging-ref
//! 负向红绿、Entry 布局漂移负向红绿（指纹硬化）。插件 dll 由
//! [`compile_plugin_library`] 供给（`arc build --dynamic` 的进程内等价物），
//! 宿主源全部走 AssemblyLoadContext 公开面状态断言——
//! Assembly.Generation/IsDisposed 均 internal，禁止绕过。
//!
//! 全部用例共享 `AssemblyLoadContext.Default` 单例（构造器私有），故每个
//! case 使用独立插件名隔离 `_loaded` 键，且结束时必须把自身加载的插件
//! 卸载干净（防污染后续 case）。

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

/// 插件目录绝对路径。统一正斜杠：宿主源字符串无反斜杠转义问题，且
/// 探针 File.Exists 与 rt 层 GetFullPathNameW/LoadLibraryExW 对正斜杠透明。
#[cfg(feature = "full-rt")]
fn plugins_dir(batch: &str) -> String {
    let root = workspace_root().to_string_lossy().replace('\\', "/");
    format!("{root}/target/arc-tests/{batch}-plugins")
}

#[cfg(feature = "full-rt")]
const PLUGIN_BODY: &str = r#"namespace PluginU5;

public class Probe
{
    public string Tag() { return "u5"; }
}
"#;

/// Entry 端到端插件：顶层无参 `Entry` 函数（RFC 017 M2 wrapper 准入条件：
/// 函数名 Entry + 返回 Named 类型）导出 `__arc_entry__{TR_id}` C ABI wrapper。
/// 返回类型必须为自定义 class——`string` 是独立 TypeId 变体不走 wrapper，
/// 且 class 走 ArcHeader* 透传（零包装）。宿主侧按裸类名重建同形布局
/// （TR_id 取裸名 FNV-1a，跨程序集确定性对齐）。
#[cfg(feature = "full-rt")]
const PLUGIN_ENTRY_BODY: &str = r#"namespace PluginU5;

public class U5EntryPayload
{
    public string Tag;
}

public U5EntryPayload Entry()
{
    U5EntryPayload payload = new U5EntryPayload();
    payload.Tag = "u5-entry-ok";
    return payload;
}
"#;

/// 布局漂移负向插件：与宿主同名 `U5MutantPayload` 但**多一个字段**
/// （2 字段 vs 宿主 1 字段）——FNV-1a-32 类型 id 相同（同名），布局指纹
/// （FNV-1a-64）必异。指纹硬化的负向红绿：符号不匹配 → 加载期显式
/// `EntryPointNotFoundException`，绝不以旧形态（符号只含类型 id）静默
/// 错配（旧形态下宿主按 1 字段布局读 2 字段插件对象 = 内存越界读）。
#[cfg(feature = "full-rt")]
const PLUGIN_MUTANT_BODY: &str = r#"namespace PluginU5;

public class U5MutantPayload
{
    public string Tag;
    public int Extra;
}

public U5MutantPayload Entry()
{
    U5MutantPayload payload = new U5MutantPayload();
    payload.Tag = "u5-mutant";
    payload.Extra = 7;
    return payload;
}
"#;

/// `llvm-readobj` 路径（与 codegen `shared_runtime::llvm_nm_path` 同序探测）。
#[cfg(feature = "full-rt")]
fn llvm_readobj_path() -> String {
    if cfg!(windows) {
        for p in [
            r"C:\Program Files\LLVM\bin\llvm-readobj.exe",
            r"C:\Program Files (x86)\LLVM\bin\llvm-readobj.exe",
        ] {
            if std::path::Path::new(p).exists() {
                return p.into();
            }
        }
    }
    "llvm-readobj".into()
}

/// 读取 PE 导入表的全部依赖 dll 名（`Import { Name: xxx.dll }` 块）。
#[cfg(feature = "full-rt")]
fn pe_import_dll_names(path: &std::path::Path) -> Vec<String> {
    let output = std::process::Command::new(llvm_readobj_path())
        .arg("--coff-imports")
        .arg(path)
        .output()
        .expect("run llvm-readobj --coff-imports");
    assert!(
        output.status.success(),
        "llvm-readobj --coff-imports failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let mut names = std::collections::BTreeSet::new();
    for cap in split_name_lines(&text) {
        names.insert(cap);
    }
    names.into_iter().collect()
}

/// 读取 PE 导入表的全部导入符号名（`Symbol: xxx (ordinal)` 行）。
#[cfg(feature = "full-rt")]
fn pe_import_symbol_names(path: &std::path::Path) -> Vec<String> {
    let output = std::process::Command::new(llvm_readobj_path())
        .arg("--coff-imports")
        .arg(path)
        .output()
        .expect("run llvm-readobj --coff-imports");
    assert!(
        output.status.success(),
        "llvm-readobj --coff-imports failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let mut names = std::collections::BTreeSet::new();
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("Symbol: ") {
            if let Some(name) = rest.split_whitespace().next() {
                names.insert(name.to_string());
            }
        }
    }
    names.into_iter().collect()
}

/// 读取 PE 导出表的全部导出符号名（`Name: xxx` 行）。
#[cfg(feature = "full-rt")]
fn pe_export_symbol_names(path: &std::path::Path) -> Vec<String> {
    let output = std::process::Command::new(llvm_readobj_path())
        .arg("--coff-exports")
        .arg(path)
        .output()
        .expect("run llvm-readobj --coff-exports");
    assert!(
        output.status.success(),
        "llvm-readobj --coff-exports failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let mut names = std::collections::BTreeSet::new();
    for cap in split_name_lines(&text) {
        names.insert(cap);
    }
    names.into_iter().collect()
}

/// 提取 `Name: <token>` 行的 token（导入/导出目录输出共用键形态）。
#[cfg(feature = "full-rt")]
fn split_name_lines(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("Name: ") {
            if let Some(name) = rest.split_whitespace().next() {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// RFC 017 阶段一验收：插件产物「导入引用」符号面断言。
///
/// debug=false 的 PE 镜像无 COFF 符号表（lld-link 仅 /DEBUG 时写入，
/// llvm-nm 报 no symbols——红基线实证），判据取导入/导出目录
/// （llvm-readobj，不依赖符号表）：
/// - 正向铁证：导入表含 arc_runtime.dll 且导入 rt_* 符号——旧内嵌形态
///   导入表仅系统库（KERNEL32/ntdll，实证），导入引用路径必经此形态；
/// - 负向：导出面无 rt_*——rt 机器码不得以导出形态回流插件映像；
/// - 导出健康：__arc_package_meta 在列（MSVC /EXPORT: 注入坑位守护，
///   依赖递归解析依赖此符号）。
///
/// ELF 走 --needed-libs 判据；Mach-O 判据待补（本批宿主测试当前仅
/// Windows/Linux 运行面）。
#[cfg(feature = "full-rt")]
#[test]
fn plugin_artifact_is_import_reference_only() {
    let batch = "u5_dynamic_load_batch";
    let dll = compile_plugin_library(batch, "plugin_u5_symface", PLUGIN_BODY, &[]);

    if cfg!(target_os = "macos") {
        // Mach-O 导入面（load commands）判据待补——见上方测试注释。
        return;
    }

    if cfg!(target_os = "windows") {
        let imports = pe_import_dll_names(&dll);
        assert!(
            imports
                .iter()
                .any(|n| n.eq_ignore_ascii_case("arc_runtime.dll")),
            "plugin must import the shared runtime, got imports: {imports:?}"
        );
        let symbols = pe_import_symbol_names(&dll);
        assert!(
            symbols.iter().any(|s| s.starts_with("rt_")),
            "plugin must import rt_* ABI symbols from the shared runtime, got: {symbols:?}"
        );
    } else {
        // ELF：DT_NEEDED 列出 SONAME（arc_runtime.so，链接期固定）。
        let output = std::process::Command::new(llvm_readobj_path())
            .arg("--needed-libs")
            .arg(&dll)
            .output()
            .expect("run llvm-readobj --needed-libs");
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains("libarc_runtime.so"),
            "plugin must need the shared runtime, got:\n{text}"
        );
    }

    let exports = pe_export_symbol_names(&dll);
    assert!(
        !exports.iter().any(|s| s.starts_with("rt_")),
        "plugin must not export rt_* symbols (rt machine code must not leak into the plugin image), got: {exports:?}"
    );
    assert!(
        exports.iter().any(|s| s == "__arc_package_meta"),
        "plugin must keep the __arc_package_meta export (dependency resolution), got: {exports:?}"
    );
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_dynamic_load_batch() {
    let batch = "u5_dynamic_load_batch";
    // 插件供给先于宿主批：宿主运行时探针按文件命中，编译期无插件感知。
    compile_plugin_library(batch, "plugin_u5_race", PLUGIN_BODY, &[]);
    compile_plugin_library(batch, "plugin_u5_reload", PLUGIN_BODY, &[]);
    compile_plugin_library(batch, "plugin_u5_weak", PLUGIN_BODY, &[]);
    compile_plugin_library(batch, "plugin_u5_cancel", PLUGIN_BODY, &[]);
    compile_plugin_library(batch, "plugin_u5_dep_a", PLUGIN_BODY, &[]);
    compile_plugin_library(batch, "plugin_u5_dep_b", PLUGIN_BODY, &["plugin_u5_dep_a"]);
    compile_plugin_library(batch, "plugin_u5_hang", PLUGIN_BODY, &[]);
    compile_plugin_library(batch, "plugin_u5_entry", PLUGIN_ENTRY_BODY, &[]);
    compile_plugin_library(batch, "plugin_u5_mutant", PLUGIN_MUTANT_BODY, &[]);

    let dir = plugins_dir(batch);
    let results = assert_compiles_and_runs_batch(
        batch,
        &[
            ("u5_concurrent_load_unload_race", &race_case(&dir)),
            ("u5_reload_generation_switch", &reload_case(&dir)),
            ("u5_weak_slot_neutralized_on_unload", &weak_case(&dir)),
            ("u5_unloading_cancel_keeps_active", &cancel_case(&dir)),
            ("u5_transitive_dependency_recursive", &dep_case(&dir)),
            // 卸载顺序护栏负向红绿：依赖方在载时先卸被依赖方必须拒载
            // （E_UNLOAD_DEPENDED）。红基线 = 静默卸载成功。
            ("u5_unload_order_guard", &unload_order_guard_case(&dir)),
            // 缺陷⑦最小复现置于 hang 之前：单进程批内真实触发 throw 的 case
            // 会以 AV 终止进程（本窗口实证），min 不依赖插件即可稳定复现
            // EH 缺陷；hang 依赖「Unload 内部 throw + 用户 catch」，受阻于
            // 同一缺陷，原断言保留不弱化（禁止为绿绕开 throw 路径）。
            ("u5_throw_catch_min", &throw_catch_min_case()),
            ("u5_hanging_ref_hard_error", &hang_case(&dir)),
            // Entry 端到端（RFC 017 阶段一验收缺口②）：加载 → Entry<T>()
            // 间接调用 → 值透传断言 → 卸载全链路。置于 hang 之后——正路径
            // 不触发 throw，不受缺陷⑦ EH 链干扰。
            ("u5_entry_call_roundtrip", &entry_case(&dir)),
            // 布局漂移负向红绿（指纹硬化验收）：同名异构 → 加载期显式
            // EntryPointNotFound。同 entry_case 置于批尾——throw 路径，
            // 隔在全部正路径之后。
            ("u5_entry_layout_drift_detected", &drift_case(&dir)),
        ],
    );
    assert_all_passed(batch, &results);
}

// ============================================================
// 诊断对照：纯 throw/catch 最小用例（不涉及插件）
// ============================================================
//
// hang_case 死点锁定在 Unload#1 内部（STEP:2-held 后无 STEP:3），且整个批
// 协议史上只有 hang_case 真实触发 throw→unwind→catch 链（其余 case 的
// try/catch 只生成 EH 表、catch 体从未执行）。本用例在宿主源内直接
// throw/catch，隔离「EH 机制缺陷」与「插件卸载路径缺陷」两个假设：若本
// 用例同样死亡 → EH codegen/运行时缺陷（与插件无关）；若通过而 hang 死 →
// 焦点回到 Unload 抛异常路径的上下文差异。

#[cfg(feature = "full-rt")]
fn throw_catch_min_case() -> String {
    r#"using Arc;

void Main()
{
    Console.WriteLine("STEP:1-before-try");
    bool caught = false;
    string detail = "";
    try
    {
        Console.WriteLine("STEP:2-before-throw");
        throw new InvalidOperationException("E_TEST_MIN_THROW");
    }
    catch (InvalidOperationException e)
    {
        Console.WriteLine("STEP:3-in-catch");
        caught = true;
        detail = e.Message;
    }
    Console.WriteLine("STEP:4-after-catch");
    if (!caught) { Console.WriteLine("ARC_CASE:u5_throw_catch_min:FAIL:no-catch"); return; }
    if (detail.IndexOf("E_TEST_MIN_THROW") < 0) { Console.WriteLine("ARC_CASE:u5_throw_catch_min:FAIL:msg=" + detail); return; }
    Console.WriteLine("ARC_CASE:u5_throw_catch_min:PASS");
}
"#
    .to_string()
}

// ============================================================
// 用例 1：并发 load/unload 竞态（§2.5-U1）
// ============================================================
//
// 确定性构造：Semaphore 屏障放行 4 线程同拍撞入 Load——rt_library CAS
// 注册表的竞态窗口在微秒级，4 线程屏障后同拍进入即必然交叠；Wait(5000)
// 仅作到达屏障的兜底，非 sleep 赌时序。线程各自 Load 同一路径（rt 层允许多
// 代数并存），错误收集进 ConcurrentQueue；主线程 Join 后断言零错误、四个
// Assembly 齐活，再逐个 Unload 验证卸载路径同样无竞态、最终状态归零。

#[cfg(feature = "full-rt")]
fn race_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Collections.Concurrent;
using Arc.Runtime;
using Arc.Threading;

void RaceWorker(string tag, string pluginPath, Semaphore gate, ConcurrentQueue<string> errs, ConcurrentQueue<Assembly> loaded)
{{
    if (!gate.Wait(5000)) {{ errs.Enqueue(tag + ":barrier-timeout"); return; }}
    try
    {{
        AssemblyLoadContext alc = AssemblyLoadContext.Default;
        Assembly asm = alc.Load(pluginPath);
        if (asm == null) {{ errs.Enqueue(tag + ":load-null"); return; }}
        loaded.Enqueue(asm);
    }}
    catch (IOException e)
    {{
        errs.Enqueue(tag + ":io:" + e.Message);
    }}
}}

void Main()
{{
    string pluginsDir = "{dir}";
    string pluginPath = pluginsDir + "/plugin_u5_race.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    Semaphore gate = new Semaphore(0, 4);
    ConcurrentQueue<string> errs = new ConcurrentQueue<string>();
    ConcurrentQueue<Assembly> loaded = new ConcurrentQueue<Assembly>();
    Thread t0 = new Thread(() => RaceWorker("t0", pluginPath, gate, errs, loaded));
    Thread t1 = new Thread(() => RaceWorker("t1", pluginPath, gate, errs, loaded));
    Thread t2 = new Thread(() => RaceWorker("t2", pluginPath, gate, errs, loaded));
    Thread t3 = new Thread(() => RaceWorker("t3", pluginPath, gate, errs, loaded));
    t0.Start();
    t1.Start();
    t2.Start();
    t3.Start();
    gate.Release(4);
    t0.Join();
    t1.Join();
    t2.Join();
    t3.Join();
    if (errs.Count != 0)
    {{
        string first = "";
        errs.TryPeek(out first);
        Console.WriteLine("ARC_CASE:u5_concurrent_load_unload_race:FAIL:errs=" + first);
        return;
    }}
    if (loaded.Count != 4)
    {{
        Console.WriteLine("ARC_CASE:u5_concurrent_load_unload_race:FAIL:loaded=" + loaded.Count);
        return;
    }}
    alc.UnloadAll();
    if (alc.GetLoadedAssembly(pluginPath) != null)
    {{
        Console.WriteLine("ARC_CASE:u5_concurrent_load_unload_race:FAIL:cleanup");
        return;
    }}
    Console.WriteLine("ARC_CASE:u5_concurrent_load_unload_race:PASS");
}}
"#
    )
}

// ============================================================
// 用例 2：同路径重载 → 代数切换（§2.5-U2）
// ============================================================
//
// rt_library.c 契约：同路径重复加载获得新代数；tombstone 后旧代数判无效。
// 顺序化重载（先卸后载）避开 ALC 层 _loaded 路径键的交叠覆盖争议，专注
// 公开面断言：旧句柄 HoldReference true→false 翻转（代数退化判定），
// 新句柄独立有效且为新对象——两代数互不扰动即完全隔离。

#[cfg(feature = "full-rt")]
fn reload_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    string pluginsDir = "{dir}";
    string pluginPath = pluginsDir + "/plugin_u5_reload.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    Assembly first = alc.Load(pluginPath);
    if (first == null) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:first-load-null"); return; }}
    if (!alc.HoldReference(first)) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:first-hold"); return; }}
    if (alc.GetReferenceCount(first) != 1) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:first-count"); return; }}
    if (!alc.ReleaseReference(first)) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:first-release"); return; }}
    alc.Unload(first);
    bool heldAfterUnload = alc.HoldReference(first);
    if (heldAfterUnload) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:old-generation-alive"); return; }}
    Assembly second = alc.Load(pluginPath);
    if (second == null) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:second-load-null"); return; }}
    if (second == first) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:same-object"); return; }}
    if (!alc.HoldReference(second)) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:new-generation-dead"); return; }}
    if (!alc.ReleaseReference(second)) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:second-release"); return; }}
    alc.Unload(second);
    if (alc.HoldReference(second)) {{ Console.WriteLine("ARC_CASE:u5_reload_generation_switch:FAIL:second-not-tombstoned"); return; }}
    Console.WriteLine("ARC_CASE:u5_reload_generation_switch:PASS");
}}
"#
    )
}

// ============================================================
// 用例 3：卸载中访问弱槽 → 中和生效（§2.5-U3）
// ============================================================
//
// 弱槽挂到插件代数上（RegisterWeakReference），卸载时 rt 层确定性中和；
// guarded.TryGet() 必须为 null——对象本身仍被局部变量强引用，未中和时
// TryGet 应返回非 null，故 null 只能来自中和路径。bystander（未登记弱槽）
// 作为对照组，排除「TryGet 恒 null」的假绿。

#[cfg(feature = "full-rt")]
fn weak_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

class U5WeakPayload
{{
}}

void Main()
{{
    string pluginsDir = "{dir}";
    string pluginPath = pluginsDir + "/plugin_u5_weak.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    Assembly asm = alc.Load(pluginPath);
    if (asm == null) {{ Console.WriteLine("ARC_CASE:u5_weak_slot_neutralized_on_unload:FAIL:load"); return; }}
    if (!alc.HoldReference(asm)) {{ Console.WriteLine("ARC_CASE:u5_weak_slot_neutralized_on_unload:FAIL:hold"); return; }}
    U5WeakPayload obj = new U5WeakPayload();
    Weak<U5WeakPayload> guarded = new Weak<U5WeakPayload>(obj);
    Weak<U5WeakPayload> bystander = new Weak<U5WeakPayload>(obj);
    if (!alc.RegisterWeakReference(asm, guarded)) {{ Console.WriteLine("ARC_CASE:u5_weak_slot_neutralized_on_unload:FAIL:register"); return; }}
    if (!alc.ReleaseReference(asm)) {{ Console.WriteLine("ARC_CASE:u5_weak_slot_neutralized_on_unload:FAIL:release"); return; }}
    alc.Unload(asm);
    if (guarded.TryGet() != null) {{ Console.WriteLine("ARC_CASE:u5_weak_slot_neutralized_on_unload:FAIL:guarded-not-neutralized"); return; }}
    if (bystander.TryGet() == null) {{ Console.WriteLine("ARC_CASE:u5_weak_slot_neutralized_on_unload:FAIL:bystander-neutralized"); return; }}
    Console.WriteLine("ARC_CASE:u5_weak_slot_neutralized_on_unload:PASS");
}}
"#
    )
}

// ============================================================
// 用例 4：OnUnloading Cancel 路径（§2.5-U4）
// ============================================================
//
// 派生 DefaultAssemblyLifecycle 覆写 OnUnloading 置 Cancel（RFC 006 默认
// 虚 dispatch，同签名即覆写）。Cancel 后 Unload 直接 return：登记表不移除、
// 代数不退化（stillListed && stillValid）。随后同一钩子第二次放行，真卸载
// 成功——验证 Cancel 只是推迟而非破坏卸载语义。
//
// 写法约束（实证自 L2 迭代，缺陷⑤详见报告）：入口包 new DefaultAssembly
// Lifecycle() 直接注入 std 侧 _lifecycle 字段后，Unload 分派链 AV（实验 B：
// SetLifecycle(基类实例) → Load 成功 → Unload 崩，无任何错误输出）。故：
// ① 全程单一派生类实例（入口包构造，分派已实证可用），用调用计数器让
// 第二次 OnUnloading 放行，替代「换回默认基类实例」；② 四钩子全覆写，
// 分派目标全部落在入口包方法体，避开未覆写槽位落 std 方法体的未实证链；
// ③ 代数有效性断言用 HoldReference/ReleaseReference 配对（HoldReference
// 会 ledger++，配对释放保净值不变——裸调用会抬高计数使后续真卸载必抛
// E_UNLOAD_HANGING_REF）。

#[cfg(feature = "full-rt")]
fn cancel_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

class U5CancelLifecycle : DefaultAssemblyLifecycle
{{
    private int _unloadingCalls;
    public string? OnResolving(AssemblyResolvingArgs args)
    {{
        return args.RequestPath;
    }}
    public void OnLoaded(AssemblyLoadArgs args)
    {{
    }}
    public void OnUnloading(AssemblyUnloadArgs args)
    {{
        _unloadingCalls = _unloadingCalls + 1;
        if (_unloadingCalls > 1) {{ return; }}
        args.Cancel = true;
    }}
    public void OnUnloaded(AssemblyUnloadedArgs args)
    {{
    }}
}}

void Main()
{{
    string pluginsDir = "{dir}";
    string pluginPath = pluginsDir + "/plugin_u5_cancel.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    alc.SetLifecycle(new U5CancelLifecycle());
    Assembly asm = alc.Load(pluginPath);
    if (asm == null) {{ Console.WriteLine("ARC_CASE:u5_unloading_cancel_keeps_active:FAIL:load"); return; }}
    if (!alc.HoldReference(asm)) {{ Console.WriteLine("ARC_CASE:u5_unloading_cancel_keeps_active:FAIL:hold"); return; }}
    alc.Unload(asm);
    if (alc.GetLoadedAssembly(pluginPath) == null) {{ Console.WriteLine("ARC_CASE:u5_unloading_cancel_keeps_active:FAIL:not-listed-after-cancel"); return; }}
    if (!alc.HoldReference(asm)) {{ Console.WriteLine("ARC_CASE:u5_unloading_cancel_keeps_active:FAIL:invalid-after-cancel"); return; }}
    if (!alc.ReleaseReference(asm)) {{ Console.WriteLine("ARC_CASE:u5_unloading_cancel_keeps_active:FAIL:release-pair"); return; }}
    if (!alc.ReleaseReference(asm)) {{ Console.WriteLine("ARC_CASE:u5_unloading_cancel_keeps_active:FAIL:release"); return; }}
    alc.Unload(asm);
    if (alc.GetLoadedAssembly(pluginPath) != null) {{ Console.WriteLine("ARC_CASE:u5_unloading_cancel_keeps_active:FAIL:not-unloaded-after-resume"); return; }}
    Console.WriteLine("ARC_CASE:u5_unloading_cancel_keeps_active:PASS");
}}
"#
    )
}

// ============================================================
// 用例 5：传递依赖递归加载/卸载（§2.5-U5）
// ============================================================
//
// plugin_u5_dep_b 的 __arc_package_meta 依赖键指向 plugin_u5_dep_a。
// 根 LoadByName 无 requestingAssembly，走探针路径——AddProbingPath 注入
// 插件目录；递归加载 a 时 requestingAssembly（b）所在目录即插件目录，平铺
// 布局使该解析免配探针。断言 a 出现在登记表且依赖图记录其请求方为 b
// （递归关系实证）；随后顺序卸载两个独立代数。探针返回拼接路径原样，
// _loaded 键完全可预测。

#[cfg(feature = "full-rt")]
fn dep_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    string pluginsDir = "{dir}";
    string keyA = pluginsDir + "/plugin_u5_dep_a.dll";
    string keyB = pluginsDir + "/plugin_u5_dep_b.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    alc.AddProbingPath(pluginsDir);
    Assembly b = alc.LoadByName("plugin_u5_dep_b");
    if (b == null) {{ Console.WriteLine("ARC_CASE:u5_transitive_dependency_recursive:FAIL:load-b"); return; }}
    Assembly a = alc.GetLoadedAssembly(keyA);
    if (a == null) {{ Console.WriteLine("ARC_CASE:u5_transitive_dependency_recursive:FAIL:dep-a-not-recursive"); return; }}
    string requester = alc.GetLoadedBy(keyA);
    if (requester != keyB) {{ Console.WriteLine("ARC_CASE:u5_transitive_dependency_recursive:FAIL:dep-graph=" + requester); return; }}
    alc.Unload(b);
    alc.Unload(a);
    if (alc.GetLoadedAssembly(keyA) != null) {{ Console.WriteLine("ARC_CASE:u5_transitive_dependency_recursive:FAIL:dep-a-leftover"); return; }}
    if (alc.GetLoadedAssembly(keyB) != null) {{ Console.WriteLine("ARC_CASE:u5_transitive_dependency_recursive:FAIL:dep-b-leftover"); return; }}
    Console.WriteLine("ARC_CASE:u5_transitive_dependency_recursive:PASS");
}}
"#
    )
}

// ============================================================
// 用例 5b：卸载顺序护栏负向红绿（E_UNLOAD_DEPENDED）
// ============================================================
//
// 复用 dep_a/dep_b 依赖对（dep_b 的 __arc_package_meta.Dependencies 含
// dep_a 包名）：依赖方 dep_b 在载时先卸被依赖方 dep_a → 护栏必须拒载
// （InvalidOperationException 消息含 E_UNLOAD_DEPENDED + 依赖方名单），
// 且拒载非破坏（dep_a 仍在载）。红基线：无护栏时静默卸载成功（依赖边
// 不参与 ledger 判定）→ 依赖方 dep_b 的接口分派悬垂 AV 窗口。捕获后按
// 正确序卸载（依赖方 dep_b 先、被依赖方 dep_a 后）→ 全干净 PASS。

#[cfg(feature = "full-rt")]
fn unload_order_guard_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    string pluginsDir = "{dir}";
    string keyA = pluginsDir + "/plugin_u5_dep_a.dll";
    string keyB = pluginsDir + "/plugin_u5_dep_b.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    alc.AddProbingPath(pluginsDir);
    Assembly b = alc.LoadByName("plugin_u5_dep_b");
    if (b == null) {{ Console.WriteLine("ARC_CASE:u5_unload_order_guard:FAIL:load-b"); return; }}
    Assembly a = alc.GetLoadedAssembly(keyA);
    if (a == null) {{ Console.WriteLine("ARC_CASE:u5_unload_order_guard:FAIL:dep-a-missing"); return; }}
    bool guarded = false;
    string detail = "";
    try
    {{
        alc.Unload(a);
    }}
    catch (InvalidOperationException e)
    {{
        guarded = true;
        detail = e.Message;
    }}
    if (!guarded) {{ Console.WriteLine("ARC_CASE:u5_unload_order_guard:FAIL:silent-unload-depended"); return; }}
    if (detail.IndexOf("E_UNLOAD_DEPENDED") < 0) {{ Console.WriteLine("ARC_CASE:u5_unload_order_guard:FAIL:missing-code=" + detail); return; }}
    if (alc.GetLoadedAssembly(keyA) == null) {{ Console.WriteLine("ARC_CASE:u5_unload_order_guard:FAIL:a-vanished"); return; }}
    alc.Unload(b);
    alc.Unload(a);
    if (alc.GetLoadedAssembly(keyB) != null) {{ Console.WriteLine("ARC_CASE:u5_unload_order_guard:FAIL:cleanup-b"); return; }}
    if (alc.GetLoadedAssembly(keyA) != null) {{ Console.WriteLine("ARC_CASE:u5_unload_order_guard:FAIL:cleanup-a"); return; }}
    Console.WriteLine("ARC_CASE:u5_unload_order_guard:PASS");
}}
"#
    )
}

// ============================================================
// 用例 6：hanging-ref 负向红绿（§2.5-U6）
// ============================================================
//
// HoldReference 抬高 ledger 后 Unload 必须 rc==0 → 抛
// InvalidOperationException（消息含 E_UNLOAD_HANGING_REF），绝不静默卸载：
// 若实现吞掉 rc 直接卸载，catch 不触发即 FAIL（负向红绿）。捕获后释放
// 引用再卸载，验证硬错误是「禁静默」而非「禁卸载」。

#[cfg(feature = "full-rt")]
fn hang_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

void Main()
{{
    string pluginsDir = "{dir}";
    string pluginPath = pluginsDir + "/plugin_u5_hang.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    Assembly asm = alc.Load(pluginPath);
    Console.WriteLine("STEP:1-loaded");
    if (asm == null) {{ Console.WriteLine("ARC_CASE:u5_hanging_ref_hard_error:FAIL:load"); return; }}
    if (!alc.HoldReference(asm)) {{ Console.WriteLine("ARC_CASE:u5_hanging_ref_hard_error:FAIL:hold"); return; }}
    Console.WriteLine("STEP:2-held");
    bool caught = false;
    string detail = "";
    try
    {{
        alc.Unload(asm);
    }}
    catch (InvalidOperationException e)
    {{
        caught = true;
        detail = e.Message;
    }}
    Console.WriteLine("STEP:3-after-first-unload");
    if (!caught) {{ Console.WriteLine("ARC_CASE:u5_hanging_ref_hard_error:FAIL:silent-unload"); return; }}
    if (detail.IndexOf("E_UNLOAD_HANGING_REF") < 0) {{ Console.WriteLine("ARC_CASE:u5_hanging_ref_hard_error:FAIL:missing-code=" + detail); return; }}
    if (!alc.ReleaseReference(asm)) {{ Console.WriteLine("ARC_CASE:u5_hanging_ref_hard_error:FAIL:release"); return; }}
    Console.WriteLine("STEP:4-released");
    alc.Unload(asm);
    Console.WriteLine("STEP:5-after-second-unload");
    if (alc.GetLoadedAssembly(pluginPath) != null) {{ Console.WriteLine("ARC_CASE:u5_hanging_ref_hard_error:FAIL:not-cleaned"); return; }}
    Console.WriteLine("ARC_CASE:u5_hanging_ref_hard_error:PASS");
}}
"#
    )
}

// ============================================================
// 用例 7：Entry 端到端往返（RFC 017 阶段一验收缺口②）
// ============================================================
//
// 加载 → `Assembly.Entry<U5EntryPayload>()` 强类型间接调用 → 卸载全链路。
// 调用点经 codegen 拦截降级为
// rt_library_sym(handle, "__arc_entry__{TR_id}_{TR_sig}") + void*→void*
// 裸函数指针调用；返回值走 ArcHeader* 透传（零包装），宿主按裸类名重建
// 同形布局读取字段——tag 值往返相等即双端类型身份（FNV-1a TR_id）与对象
// 布局（FNV-1a-64 TR_sig）的跨程序集对齐实证。可空返回赋非可空局部为既有
// 惯例（race_case 的 Load 同款），null 判定走 FAIL 分支。
//
// **Unload 前置条件**（隔离探针二分实证）：hanging-ref 检测覆盖
// HoldReference/ledger，不含跨界普通对象引用；引用残留时 Unload 成功、
// case 栈帧销毁时的 dec 访问已卸载段 → AV。宿主必须在 Unload 前释放
// 插件对象引用（`payload = null`，对标 C# 事件反订阅 / C 回调注销的
// 跨界资源前置清理）；「卸载残留对象 tombstone 化」为 RFC 017 后续议题。

#[cfg(feature = "full-rt")]
fn entry_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

class U5EntryPayload
{{
    public string Tag;
}}

void Main()
{{
    string pluginsDir = "{dir}";
    string pluginPath = pluginsDir + "/plugin_u5_entry.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    Assembly asm = alc.Load(pluginPath);
    if (asm == null) {{ Console.WriteLine("ARC_CASE:u5_entry_call_roundtrip:FAIL:load"); return; }}
    try
    {{
        U5EntryPayload payload = asm.Entry<U5EntryPayload>();
        if (payload == null) {{ Console.WriteLine("ARC_CASE:u5_entry_call_roundtrip:FAIL:entry-null"); return; }}
        if (payload.Tag != "u5-entry-ok") {{ Console.WriteLine("ARC_CASE:u5_entry_call_roundtrip:FAIL:tag=" + payload.Tag); return; }}
        payload = null;
        alc.Unload(asm);
        if (alc.GetLoadedAssembly(pluginPath) != null) {{ Console.WriteLine("ARC_CASE:u5_entry_call_roundtrip:FAIL:cleanup"); return; }}
        Console.WriteLine("ARC_CASE:u5_entry_call_roundtrip:PASS");
    }}
    catch (EntryPointNotFoundException e)
    {{
        Console.WriteLine("ARC_CASE:u5_entry_call_roundtrip:FAIL:entry-throw:" + e.Message);
    }}
}}
"#
    )
}

// ============================================================
// 用例 8：布局漂移负向红绿（指纹硬化验收）
// ============================================================
//
// 宿主与插件对同名 `U5MutantPayload` 各自声明**异构布局**（宿主 1 字段 /
// 插件 2 字段）：类型 id（FNV-1a-32 裸名哈希）相同，布局指纹
// （FNV-1a-64 布局闭包）必异 → 宿主构造的符号在插件导出面缺失 →
// rt_library_sym NULL → 加载期显式 `EntryPointNotFoundException`。
// 红基线：指纹未生效时符号按类型 id 匹配成功 → 宿主按 1 字段布局读
// 2 字段插件对象（silent-match 分支）——Tag 之后的越界读为静默 UB，
// 本用例把它转为显式 FAIL。

#[cfg(feature = "full-rt")]
fn drift_case(dir: &str) -> String {
    format!(
        r#"using Arc;
using Arc.Runtime;

class U5MutantPayload
{{
    public string Tag;
}}

void Main()
{{
    string pluginsDir = "{dir}";
    string pluginPath = pluginsDir + "/plugin_u5_mutant.dll";
    AssemblyLoadContext alc = AssemblyLoadContext.Default;
    Assembly asm = alc.Load(pluginPath);
    if (asm == null) {{ Console.WriteLine("ARC_CASE:u5_entry_layout_drift_detected:FAIL:load"); return; }}
    try
    {{
        U5MutantPayload payload = asm.Entry<U5MutantPayload>();
        Console.WriteLine("ARC_CASE:u5_entry_layout_drift_detected:FAIL:silent-match");
    }}
    catch (EntryPointNotFoundException e)
    {{
        alc.Unload(asm);
        if (alc.GetLoadedAssembly(pluginPath) != null) {{ Console.WriteLine("ARC_CASE:u5_entry_layout_drift_detected:FAIL:cleanup"); return; }}
        Console.WriteLine("ARC_CASE:u5_entry_layout_drift_detected:PASS");
    }}
}}
"#
    )
}
