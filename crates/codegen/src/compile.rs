//! Public compile / link entry points.

use crate::emit_role::EmitRole;
use crate::error::CodegenError;
use crate::generate_to_table::GenerateToTable;
use crate::llvm_ir;
use crate::llvm_ir::static_init_diag::StaticInitDiagnostic;
use std::path::{Path, PathBuf};

/// 包元数据——从 arc.toml [package] 节提取，嵌入动态库供运行时版本校验。
///
/// 对齐 .NET 的 AssemblyName 概念：name + version 的组合构成包的身份。
/// 宿主 AssemblyLoadContext.Load() 时通过 rt_library_get_meta() 读取此信息，
/// 实现编译期 → 运行时的类型/版本兼容性桥接。
#[derive(Clone, Debug, Default)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    pub edition: String,
    /// 运行时传递依赖（RFC 017 M3 gap ②）：依赖包名列表，嵌入 `__arc_package_meta`
    /// 的 name/version/edition 之后。空 = 无运行时依赖（保持旧 3 字段格式）。
    pub dependencies: Vec<String>,
    /// 布局指纹表（RFC 045 D8.1 状态迁移 L1）：全部自定义 Named 类型
    /// （classes + structs）的 `entry_layout_signature`，嵌入 `__arc_package_meta`
    /// 依赖段之后的第 5 字段（`Type:sig;...` 子表，无 NUL）。空 = 未物化
    /// （旧产物格式，运行时按「未知」保守处理）。
    pub layout_sigs: Vec<(String, i64)>,
}

/// 项目类型——对齐 C# 项目模型，编译期固定规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectKind {
    /// 可执行程序：必须恰好有一个 Main() 入口函数。
    Executable,
    /// 库：不要求 Main()，可有一个或多个 Entry<T>() 泛型入口函数。
    Library,
}

/// Compile MIR to a native executable via LLVM IR + clang.
///
/// 可执行项目规则（对标 C#）：
/// - 恰好一个 `Main()` 入口函数（0 个或 >1 个均为编译错误）
/// - 库项目传入 `ProjectKind::Library` 可豁免 Main() 检查
pub fn compile_module(
    fns: &[(String, mir::MirCfgBody)],
    layouts: &typeck::ProgramLayouts,
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&str>,
    release: bool,
    file_path: &str,
    source: &str,
    debug_info: bool,
    fn_spans: &std::collections::HashMap<String, ast::Span>,
    native_modules: &[ast::NativeModule],
    native_lib_paths: &[PathBuf],
    generate_to_table: &GenerateToTable,
    external_symbols: &[typeck::ExternalSymbolEntry],
    project_kind: ProjectKind,
    keep_ir: bool,
) -> Result<Vec<StaticInitDiagnostic>, CodegenError> {
    let main_fns: Vec<&str> = fns
        .iter()
        .filter(|(n, _)| llvm_ir::is_entry_fn(n))
        .map(|(n, _)| n.as_str())
        .collect();

    match main_fns.len() {
        0 if project_kind == ProjectKind::Library => {
            // 库项目不要求 Main()，直接编译
        }
        0 => {
            return Err(CodegenError::NoMain);
        }
        1 => {
            // 恰好一个 Main()，正常
        }
        n => {
            return Err(CodegenError::MultipleMain(format!(
                "found {} Main functions: {}",
                n,
                main_fns.join(", ")
            )));
        }
    }

    llvm_ir::compile_via_llvm_ir(
        fns,
        layouts,
        output,
        obj_dir,
        target,
        release,
        file_path,
        source,
        debug_info,
        fn_spans,
        native_modules,
        native_lib_paths,
        generate_to_table,
        external_symbols,
        EmitRole::MainObject,
        keep_ir,
    )
}

/// Compile MIR to a relocatable object (`.o`) — RFC 017 M3.
pub fn compile_module_to_object(
    fns: &[(String, mir::MirCfgBody)],
    layouts: &typeck::ProgramLayouts,
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&str>,
    release: bool,
    file_path: &str,
    source: &str,
    debug_info: bool,
    fn_spans: &std::collections::HashMap<String, ast::Span>,
    native_modules: &[ast::NativeModule],
    generate_to_table: &GenerateToTable,
    external_symbols: &[typeck::ExternalSymbolEntry],
    emit_role: EmitRole,
    package_meta: Option<PackageMeta>,
    keep_ir: bool,
) -> Result<Vec<StaticInitDiagnostic>, CodegenError> {
    llvm_ir::compile_to_object(
        fns,
        layouts,
        output,
        obj_dir,
        target,
        release,
        file_path,
        source,
        debug_info,
        fn_spans,
        native_modules,
        generate_to_table,
        external_symbols,
        emit_role,
        package_meta,
        keep_ir,
    )
}

/// Link pre-compiled objects + runtime into an executable.
pub fn link_objects_to_executable(
    objs: &[PathBuf],
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&str>,
    release: bool,
    debug_info: bool,
    native_modules: &[ast::NativeModule],
    native_lib_paths: &[PathBuf],
) -> Result<(), CodegenError> {
    llvm_ir::link_objects_to_executable(
        objs,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_modules,
        native_lib_paths,
    )
}

/// Compile MIR to a shared dynamic library — RFC 017 D8.
pub fn compile_module_to_dynamic_library(
    fns: &[(String, mir::MirCfgBody)],
    layouts: &typeck::ProgramLayouts,
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&str>,
    release: bool,
    file_path: &str,
    source: &str,
    debug_info: bool,
    fn_spans: &std::collections::HashMap<String, ast::Span>,
    native_modules: &[ast::NativeModule],
    native_lib_paths: &[PathBuf],
    generate_to_table: &GenerateToTable,
    external_symbols: &[typeck::ExternalSymbolEntry],
    export_symbols: &[String],
    package_meta: Option<PackageMeta>,
    keep_ir: bool,
) -> Result<Vec<StaticInitDiagnostic>, CodegenError> {
    // 输出目录自建：动态库链接前确保 bin 目录存在（对标 obj/ 的自建语义——
    // 全新项目首建时 bin/ 尚不存在，lld-link 不会自建输出目录）。
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut all_exports: Vec<String> = export_symbols.to_vec();
    // 布局指纹表（RFC 045 D8.1 状态迁移 L1）：全部自定义 Named 类型
    // （classes + structs）的 entry_layout_signature——嵌入 `__arc_package_meta`
    // 第 5 字段，供运行时跨代兼容性判定（同名类型同指纹 → 结构兼容）。
    // IndexMap 声明序即遍历序；枚举布局恒为判别值宽度、variant 无字段布局，
    // 均不参与判定（不物化）。
    let mut package_meta = package_meta;
    if let Some(ref mut pm) = package_meta {
        for name in layouts.classes.keys() {
            let sig = llvm_ir::entry_layout_signature(layouts, name.as_str());
            pm.layout_sigs.push((name.as_str().to_string(), sig as i64));
        }
        for name in layouts.structs.keys() {
            let sig = llvm_ir::entry_layout_signature(layouts, name.as_str());
            pm.layout_sigs.push((name.as_str().to_string(), sig as i64));
        }
    }
    for (name, body) in fns {
        let is_entry = name == "Entry" || name.ends_with("::Entry");
        // RFC 017 M3: 支持 0 参数 Entry() → TResult? 和 1 参数 Entry(T?) → TResult?
        // 拒绝多参数（>1）——多参数 Entry 的 C ABI 编组语义尚未定义。
        if !is_entry || body.params.len() > 1 {
            continue;
        }
        let ty_name = |ty: &ast::TypeId| -> Option<String> {
            match ty {
                ast::TypeId::Named(s) => Some(s.to_string()),
                ast::TypeId::Nullable { ref inner } => match inner.as_ref() {
                    ast::TypeId::Named(s) => Some(s.to_string()),
                    _ => None,
                },
                _ => None,
            }
        };
        let Some(tr) = ty_name(&body.ret) else {
            continue;
        };
        let tr_id = llvm_ir::type_name_to_id(&tr);
        // 布局指纹段：与 emit_entry_wrappers / 宿主 emit_call 同构（见
        // entry_layout_signature 文档）。
        let tr_sig = llvm_ir::entry_layout_signature(layouts, &tr);

        if body.params.is_empty() {
            // 无参 Entry: __arc_entry__{TR_id}_{TR_sig}
            all_exports.push(format!("__arc_entry__{tr_id}_{tr_sig}"));
        } else {
            // 单参 Entry: __arc_entry_{TP_id}_{TR_id}_{TP_sig}_{TR_sig}
            let Some(tp) = ty_name(&body.params[0].1) else {
                continue;
            };
            let tp_id = llvm_ir::type_name_to_id(&tp);
            let tp_sig = llvm_ir::entry_layout_signature(layouts, &tp);
            all_exports.push(format!("__arc_entry_{tp_id}_{tr_id}_{tp_sig}_{tr_sig}"));
        }
    }

    // RFC 017 M4: 将包元数据符号加入导出列表，使宿主可通过 rt_library_sym 读取
    if package_meta.is_some() {
        all_exports.push("__arc_package_meta".to_string());
    }

    // RFC 017 §2.3: 模块根元数据表——运行时 rt_library_load 经
    // dlsym/GetProcAddress 自动发现并登记模块根（宿主无需手动 RegisterModuleRoot）。
    // Windows PE 须经导出表可见（数据符号），故加入导出列表。
    all_exports.push("__arc_module_roots".to_string());
    all_exports.push("__arc_module_roots_count".to_string());

    // RFC 017 阶段一任务⑥：模块 init 与 dbg 表符号加入导出列表——插件 dll 改为
    // 导入引用 arc_runtime 后，rt_library_load 在加载期经 dlsym/GetProcAddress
    // 发现 __arc_module_init（触发静态初始化，对齐 main 入口语义）并登记
    // __arc_dbg_table(+count)（StackTrace 符号化 registry）。与 __arc_module_roots
    // 同属数据/代码符号导出（Windows MSVC 须显式 /EXPORT:）。
    all_exports.push("__arc_module_init".to_string());
    all_exports.push("__arc_dbg_table".to_string());
    all_exports.push("__arc_dbg_count".to_string());

    // RFC 006 A3 S5：导出静态字段全局（`@__static_<Class>_<field>`），使宿主可经
    // rt_library_sym 解析并观测库内静态状态（含惰性字段的构造时序 s_constructed、
    // 以及 class 引用槽位 `__static_E__Shared` 供根扫描验证）。Linux/macOS/MinGW
    // 默认导出全局符号（无需额外标志），Windows MSVC 须显式 `/EXPORT:`，故统一
    // 加入导出列表——与 __arc_module_roots 同属数据符号导出。
    for sf in &layouts.static_fields {
        all_exports.push(format!("__static_{}_{}", sf.class, sf.field));
    }

    // RFC 047（透明对象图迁移 · L3）：vtable 登记表符号加入导出面——条目
    // 计算与 emit_module 单点共享（vtable_registry_entries）；`.vtable.{T}`
    // 为数据符号，Windows MSVC 须显式 /EXPORT:（否则迁移时 GetProcAddress
    // 解析失败 → 保守拒绝）。external 类的 vtable 定义在别处，不在本 TU
    // registry，导出面与 emit_module 发射严格对齐。
    let external_types: std::collections::HashSet<String> = external_symbols
        .iter()
        .filter(|e| matches!(e.kind, typeck::ExternalSymbolKind::Class))
        .map(|e| e.name.clone())
        .collect();
    let registry = llvm_ir::vtable_registry_entries(layouts, &|n: &str| external_types.contains(n));
    if package_meta.as_ref().is_some_and(|pm| !pm.name.is_empty()) {
        all_exports.push("__arc_vtable_registry".to_string());
        all_exports.push("__arc_vtable_registry_count".to_string());
        for (name, _, _, _) in &registry {
            all_exports.push(format!(".vtable.{name}"));
        }
    }

    let work_dir = {
        let stem = output.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        let base = obj_dir
            .map(|d| d.to_path_buf())
            .unwrap_or_else(|| output.parent().unwrap_or(Path::new(".")).to_path_buf());
        base.join(stem)
    };
    std::fs::create_dir_all(&work_dir)?;

    let obj_path = work_dir.join("out.o");
    let diags = compile_module_to_object(
        fns,
        layouts,
        &obj_path,
        obj_dir,
        target,
        release,
        file_path,
        source,
        debug_info,
        fn_spans,
        native_modules,
        generate_to_table,
        external_symbols,
        // RFC 017 热卸载 M2 剩余项：--dynamic 共享库自含 runtime（rt_debug.o
        // 硬引用 __arc_dbg_table/__arc_dbg_count），须发射 dbg 表就地解析。
        EmitRole::DynamicLibrary,
        package_meta,
        keep_ir,
    )?;

    link_objects_to_dynamic_library(
        &[obj_path],
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_modules,
        native_lib_paths,
        &all_exports,
    )?;

    Ok(diags)
}

/// Link objects into a shared dynamic library — RFC 017 D8.
pub fn link_objects_to_dynamic_library(
    objs: &[PathBuf],
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&str>,
    release: bool,
    debug_info: bool,
    native_modules: &[ast::NativeModule],
    native_lib_paths: &[PathBuf],
    export_symbols: &[String],
) -> Result<(), CodegenError> {
    llvm_ir::link_objects_to_dynamic_library(
        objs,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_modules,
        native_lib_paths,
        export_symbols,
    )
}
