//! LLVM IR text generation backend (RFC 015 Phase A).
//!
//! Generates `.ll` text from `MirCfgBody`, invokes clang for AOT compilation.
//! This is the native LLVM backend — not a C transpile bridge. LLVM IR is the
//! compiler's canonical output; clang drives optimization (`-O0`..`-O3`) and
//! code generation via LLVM's PassManager and CodeGen.

mod arc_drop;
mod arc_optimize;
mod attr;
mod builtin_dispatch;
mod builtin_math;
mod builtin_primitive;
mod builtin_span;
mod builtin_tensor;
mod builtin_vector;
mod completeness;
mod debug_info;
mod emit_aggregate;
mod emit_async_coro;
mod emit_async_sm;
mod emit_binary;
mod emit_box;
mod emit_builtin;
mod emit_call;
mod emit_call_threading;
mod emit_cfg;
mod emit_di;
mod emit_expr_tree;
mod emit_fn;
mod emit_native_callback;
mod emit_rvalue;
mod emit_static;
pub(crate) mod emit_stubs;
mod emit_variant;
mod expr_rodata;
/// RFC 017 M4-link Phase B：跨 `.ao` 包外部符号 `declare` 发射。
mod external_decls;
mod linq_foreach;
pub(crate) mod mangle;
mod native;
mod optimize;
mod runtime_decls;
mod sb_promote;
mod shared_runtime;
mod static_init_deps;
pub(crate) mod static_init_diag;
mod string_pool;
mod types;

pub(crate) use attr::{analyze_module_nounwind, infer_user_fn_attrs, is_known_nounwind_external};
pub(crate) use emit_rvalue::{entry_layout_signature, type_name_to_id, TyVal};
pub(crate) use mangle::is_entry_fn;

use crate::CodegenError;
use crate::GenerateToTable;
use ast::{Ident, Span, TypeId};
use mir::{LocalId, MirCfgBody};
use static_init_diag::StaticInitDiagnostic;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use typeck::{ClassLayout, ProgramLayouts, StructLayout};

use mangle::{
    clang_path, crypto_native_vendor_subdir, gui_subsystem_flags, mangle_fn_name, mangle_method,
    platform_link_flags, target_os, wgpu_native_vendor_subdir, TargetOs,
};
use string_pool::{collect_string_literals, emit_string_globals, StringConstAccumulator};

use self::expr_rodata::{collect_expr_trees, emit_expr_node_type, emit_expr_tree_globals};
use self::external_decls::emit_external_decls;
use self::runtime_decls::emit_runtime_decls;
use self::types::{
    delegate_ret_type, dict_cmp_fn, dict_eq_fn, dict_hash_fn, dict_kv_is_scalar,
    dict_kv_is_user_type, dict_kv_llvm_ty, dict_kv_ptr_to_scalar, dict_kv_scalar_to_ptr,
    dict_user_eq_fn, dict_user_hash_fn, dict_value_has_equals, iface_generic_root, int_rank,
    is_delegate_type, is_iface_name, is_opaque_runtime_handle, is_primitive_value_type,
    is_unsigned_int_ty, list_arc_dec_fn, list_arc_inc_fn, list_elem_is_ref, list_elem_llvm_ty,
    list_elem_size, list_eq_fn, llvm_align_of, llvm_field_type, llvm_size_of,
    llvm_size_of_type_str, llvm_type_of, nullable_aggregate_inner, nullable_value_llvm_type,
    parse_concurrent_dict_kv, parse_concurrent_single_elem, parse_dict_enumerator_kv,
    parse_dict_kv, parse_enumerator_elem, parse_linked_list_elem, parse_linked_list_node_elem,
    parse_list_elem, parse_queue_elem, parse_set_elem, parse_sorted_dict_kv, parse_sorted_set_elem,
    parse_stack_elem, parse_tensor_elem, parse_vector_class, parse_weak_elem,
    pcc_kind_from_type_name, primitive_value_storage_llvm_type, returned_owner_local, INT_TYS,
};

/// RFC 037 §D7.2: wgpu-native vendoring 根目录（`<rt-base>/runtime-ui/wgpu-native`）。
///
/// 子目录布局：
/// - `bin/<os>/`：平台特定预编译二进制（`wgpu_native.dll` + `libwgpu_native.dll.a` on Windows）
/// - `include/`：跨平台头文件（`webgpu.h` + `wgpu.h`）
///
/// 安装态 `<sdk>/lib/rt/runtime-ui/wgpu-native`；开发态 `<repo>/crates/runtime-ui/wgpu-native`。
fn wgpu_native_vendor_root() -> PathBuf {
    crate::sdk_layout::sdk_runtime_base().join("runtime-ui/wgpu-native")
}

/// wgpu-native 平台二进制目录（`bin/<os>/`），解析序：`arc component` 管理的
/// 活动 wgpu 组件 → vendored 根。
///
/// 组件态 `<tools>/components/wgpu/<ver>/bin/<os>`（`arc component install wgpu`
/// 归一化为与 vendored 一致的 `bin/<os>/` 子布局）；vendored 态
/// `<rt-base>/runtime-ui/wgpu-native/bin/<os>`。目录存在才返回 `Some`；
/// 单一解析序（lib path 注入 / DLL 复制 / 链接库判定共用），避免双轨。
fn wgpu_native_bin_dir(target: Option<&str>) -> Option<PathBuf> {
    let subdir = wgpu_native_vendor_subdir(target)?;
    if let Some(active) = crate::sdk_layout::component_active_dir("wgpu") {
        let component_bin = active.join("bin").join(subdir);
        if component_bin.is_dir() {
            return Some(component_bin);
        }
    }
    let vendor_bin = wgpu_native_vendor_root().join("bin").join(subdir);
    vendor_bin.is_dir().then_some(vendor_bin)
}

/// RFC 026 M1: vendored 密码学底座根目录（`<rt-base>/runtime-crypto`）。
///
/// 子目录布局：
/// - `bin/<os>/`：平台特定预构建二进制（`crypto_native.dll` + `crypto_native.lib` +
///   `libcrypto_native.dll.a` on Windows），由 `scripts/fetch-boringssl-native.ps1`
///   生成（mbedTLS 4.1.1 + Arc-authored `rt_crypto_*` ABI shim）。
///
/// 安装态 `<sdk>/lib/rt/runtime-crypto`；开发态 `<repo>/crates/runtime-crypto`。
fn crypto_native_vendor_root() -> PathBuf {
    crate::sdk_layout::sdk_runtime_base().join("runtime-crypto")
}

/// 用户裁决简化（2026-08-03）：`library` 相对路径基准 = **执行程序根目录**。
///
/// 将契约内相对 `library` 路径按 `base_dir`（`-o` 输出可执行文件所在目录）
/// 解析为绝对路径；绝对路径原样保留。使符号验证 / 链接标志 / 运行时候选
/// 统一使用同一绝对基准（loader 不再按 workspace 根解析）。
fn resolve_module_library_paths(
    modules: &[ast::NativeModule],
    base_dir: &Path,
) -> Vec<ast::NativeModule> {
    modules
        .iter()
        .map(|m| {
            let mut m = m.clone();
            if let Some(lib) = &m.library {
                if !lib.is_absolute() {
                    m.library = Some(base_dir.join(lib));
                }
            }
            m
        })
        .collect()
}

/// 计算链接期有效的 native lib 搜索路径，自动注入 wgpu-native vendor lib 目录。
///
/// 顺序：用户配置（`ani-native-lib` + CLI `--ani-native-lib`）→ per-module 契约
/// `library` 目录（RFC 016 M4 多库体系隔离）→ wgpu-native vendor lib 目录。
///
/// 只要目标平台已 vendoring 且目录存在，就注入 `<vendor>/bin/<os>/`：
/// - `arc build` 路径：`native_modules` 含 `wgpu_native`，触发 lib path 注入
/// - `arc test` 路径：`linker.rs::link_test_harness` 显式传入空 `native_modules`
///   （不重新解析 .ani 契约），但 `rt_wgpu_native.o` 始终在 runtime_objs 中
///   编译并链接，因此仍需注入 vendor lib path 以解析 wgpu-native C API 符号
///   （wgpuCreateInstance 等来自 `wgpu_native.lib`）。
fn effective_native_lib_paths(
    native_modules: &[ast::NativeModule],
    user_lib_paths: &[PathBuf],
    target: Option<&str>,
) -> Vec<PathBuf> {
    if target.map(mangle::is_wasm_triple).unwrap_or(false) {
        return user_lib_paths.to_vec();
    }
    let mut paths: Vec<PathBuf> = user_lib_paths.to_vec();
    // RFC 016 M4：per-module 契约 `library` 目录追加为链接器 -L 标志，
    // 使每个库体系能在自身目录解析库文件，无需混入同一目录。
    for module in native_modules {
        if let Some(dir) = &module.library {
            if !paths.contains(dir) {
                paths.push(dir.clone());
            }
        }
    }
    if let Some(vendor_lib) = wgpu_native_bin_dir(target) {
        paths.push(vendor_lib);
    } else {
        let fallback_lib = crate::sdk_layout::sdk_runtime_base().join("runtime-ui/wgpu-native");
        if fallback_lib.join("wgpu_native.lib").exists() {
            paths.push(fallback_lib);
        }
    }
    // RFC 026 M1: vendored crypto base（`crates/runtime-crypto/bin/<os>/`）。
    // 存在时注入链接器 -L 路径以解析 `rt_crypto_*`（AEAD/RSA/X25519）符号。
    if let Some(subdir) = crypto_native_vendor_subdir(target) {
        let vendor_lib = crypto_native_vendor_root().join("bin").join(subdir);
        if vendor_lib.exists() && !paths.contains(&vendor_lib) {
            paths.push(vendor_lib);
        }
    }
    paths
}

/// RFC 017 产物域（U3 dll 单副本，UX 迭代评审 §2.3）：vendored native dll 落位。
///
/// 旧机制把 ~75 MB 运行时 dll 逐项目复制进每个 bin/（examples 实测 dll 占中间
/// 产物 74.7 MB）；现改为：首建复制进 `$ARC_HOME/cache/<dll 原名>`（全局单副本），
/// 项目 bin/ 以硬链接引用缓存副本——硬链接同卷零额外磁盘占用；跨卷或文件系统
/// 不支持硬链接时回退 `fs::copy`（构建不破）。
///
/// 自愈与确定性：缓存缺失或内容与源不一致（版本更新）时重建，已存在且逐字节
/// 一致时跳过全部 I/O。缓存层不可用（无写权限）时降级为源直连复制——空间优化是
/// 尽力而为，产物正确性永远优先。命中判据为内容逐字节一致（len 快速前置）：
/// 「dll 版本更新几乎必然改变长度」的旧 len 判据经批测取证实证不成立——runtime
/// 数行级改动重编后 dll 长度可恰好不变，len 命中会让产物静默服役旧 runtime；
/// mtime 不参与（`fs::copy` 是否保留修改时间平台相关，引入 mtime 会让部分平台
/// 每次构建都重建缓存）。

/// 两文件内容是否逐字节一致（len 相等的快速前置由调用方完成）。
///
/// stage_vendored_dll 的落位判定不得以 len 相等替代内容一致：runtime 重编后
/// dll 长度可能恰好不变（改动仅数行时高概率），len 判定会让产物目录继续服役
/// 旧 runtime——批测取证实证：rt_task.c 加桩重链后批目录 dll 未更新，新桩
/// 静默（arc-runtime 单副本共享契约下这是所有产物的正确性问题，不只是取证）。
fn staged_dll_matches(src_dll: &Path, candidate: &Path) -> bool {
    if fs::metadata(candidate).map(|d| d.len()).ok()
        != fs::metadata(src_dll).map(|m| m.len()).ok()
    {
        return false;
    }
    match (fs::read(candidate), fs::read(src_dll)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn stage_vendored_dll(src_dll: &Path, dest: &Path) {
    let Some(file_name) = src_dll.file_name() else {
        return;
    };
    let cache_dll = crate::sdk_layout::native_cache_dir().join(file_name);
    let effective = if staged_dll_matches(src_dll, &cache_dll) {
        cache_dll
    } else {
        if let Some(parent) = cache_dll.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::copy(src_dll, &cache_dll).is_ok() {
            cache_dll
        } else {
            src_dll.to_path_buf()
        }
    };
    // dest 层：与 effective 内容一致即已落位（硬链接同 inode 时逐字节必等，
    // read 快速路径；内容漂移则重链到新副本）。
    if staged_dll_matches(&effective, dest) {
        return;
    }
    // 硬链接要求目标不存在：先清位（旧副本可能是普通文件或残留链接）。
    let _ = fs::remove_file(dest);
    if fs::hard_link(&effective, dest).is_ok() {
        return;
    }
    // 跨卷 / 不支持硬链接 → 回退复制（RFC 017：回退保持构建不破）。
    if let Err(e) = fs::copy(&effective, dest) {
        eprintln!(
            "warning: stage_vendored_dll failed: {} -> {}: {e}",
            effective.display(),
            dest.display()
        );
    }
}

/// Windows 链接后落位 `wgpu_native.dll` 到可执行文件同目录（best-effort）。
///
/// 运行时 DLL 必须与 .exe 同目录才能被加载；经 [`stage_vendored_dll`] 走
/// 全局单副本缓存。失败不阻断构建。
fn copy_wgpu_native_dll_if_needed(output: &Path, target: Option<&str>) {
    // std P3：同 copy_crypto_native_dll_if_needed 的门卫修复（Host+cfg!(windows)）。
    if !mangle::is_windows_target(target) {
        return;
    }
    let Some(bin_dir) = wgpu_native_bin_dir(target) else {
        return;
    };
    let dll = bin_dir.join("wgpu_native.dll");
    if !dll.exists() {
        return;
    }
    let Some(dest_dir) = output.parent() else {
        return;
    };
    // 防御：若 output 为裸文件名（无目录部分），parent() 返回空路径 ""，
    // join 会产出相对路径解析到 CWD。拒绝此情况避免 DLL 散落。
    if dest_dir.as_os_str().is_empty() {
        return;
    }
    stage_vendored_dll(&dll, &dest_dir.join("wgpu_native.dll"));
}

/// 确保链接库列表包含 `wgpu_native`（当 vendor lib 存在时）。
///
/// `rt_wgpu_native.c` 始终在 `rt_sources` 中编译并链接到 runtime_objs，
/// 因此任何链接 runtime 的二进制都必须链接 `wgpu_native.lib`。
/// 但 `native_link_libs(modules)` 仅基于 `native_modules` 计算——
/// `arc test` 路径显式传空 `native_modules`（不重新解析 .ani），
/// 会导致 `-lwgpu_native` 缺失，链接报 undefined symbol: wgpuCreateInstance 等。
///
/// 此函数在 vendor lib 存在时确保 `wgpu_native` 在列表中（去重）。
fn filter_wasm_native_link_libs(libs: Vec<String>, target: Option<&str>) -> Vec<String> {
    if !target.map(mangle::is_wasm_triple).unwrap_or(false) {
        return libs;
    }
    libs.into_iter().filter(|l| l != "wgpu_native").collect()
}

fn ensure_wgpu_native_link_lib(libs: Vec<String>, target: Option<&str>) -> Vec<String> {
    if target.map(mangle::is_wasm_triple).unwrap_or(false) {
        return filter_wasm_native_link_libs(libs, target);
    }
    let Some(_vendor_lib) = wgpu_native_bin_dir(target) else {
        return libs;
    };
    if libs.iter().any(|s| s == "wgpu_native") {
        return libs;
    }
    let mut libs = libs;
    libs.push("wgpu_native".to_string());
    libs
}

/// RFC 026 M1: 确保链接库列表包含 `crypto_native`（当 vendored 底座存在时）。
///
/// `std/Security/Crypto/*` facade 经 codegen 直射 `@rt_crypto_*` ABI 符号；
/// 这些符号定义在 vendored `crypto_native.lib`（`crates/runtime-crypto/bin/<os>/`）。
/// 底座缺失时（未入库平台）不注入，e2e 依 clang/DLL 软跳过纪律门禁。
fn ensure_crypto_native_link_lib(libs: Vec<String>, target: Option<&str>) -> Vec<String> {
    let Some(subdir) = crypto_native_vendor_subdir(target) else {
        return libs;
    };
    let vendor_lib = crypto_native_vendor_root().join("bin").join(subdir);
    if !vendor_lib.exists() {
        return libs;
    }
    if libs.iter().any(|s| s == "crypto_native") {
        return libs;
    }
    let mut libs = libs;
    libs.push("crypto_native".to_string());
    libs
}

/// RFC 026 M1: Windows 链接后落位 `crypto_native.dll` 到可执行文件同目录
///（best-effort；运行时 DLL 必须与 .exe 同目录才能被加载）。经
/// [`stage_vendored_dll`] 走全局单副本缓存。
///
/// 门卫必须用 [`mangle::is_windows_target`]（与 [`copy_wgpu_native_dll_if_needed`]
/// 同谓词）：批测（arc-tests）进程内编译传 `target=None`，`target_os` 落
/// `TargetOs::Host`——若此处用 `matches!(target_os(..), Windows)` 窄判，
/// Windows 宿主上的批测会跳过落位，产物导入 `crypto_native.dll`（Arc.Net 包
/// 经源码合并编入 TLS 面，链接器写入导入表）却缺 DLL → 0xC0000135
/// STATUS_DLL_NOT_FOUND 起跑即死，批测全部 case「未执行」（l2_net_batch
/// 边界崩溃实证）。
fn copy_crypto_native_dll_if_needed(output: &Path, target: Option<&str>) {
    if !mangle::is_windows_target(target) {
        return;
    }
    let Some(subdir) = crypto_native_vendor_subdir(target) else {
        return;
    };
    let dll = crypto_native_vendor_root()
        .join("bin")
        .join(subdir)
        .join("crypto_native.dll");
    if !dll.exists() {
        return;
    }
    let Some(dest_dir) = output.parent() else {
        return;
    };
    if dest_dir.as_os_str().is_empty() {
        return;
    }
    stage_vendored_dll(&dll, &dest_dir.join("crypto_native.dll"));
}

/// Entry point: compile MIR functions to an executable via LLVM IR.
pub fn compile_via_llvm_ir(
    fns: &[(String, MirCfgBody)],
    layouts: &ProgramLayouts,
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&str>,
    release: bool,
    file_path: &str,
    source: &str,
    debug_info: bool,
    fn_spans: &HashMap<String, Span>,
    native_modules: &[ast::NativeModule],
    native_lib_paths: &[PathBuf],
    generate_to_table: &GenerateToTable,
    external_symbols: &[typeck::ExternalSymbolEntry],
    emit_role: crate::EmitRole,
    keep_ir: bool,
) -> Result<Vec<StaticInitDiagnostic>, CodegenError> {
    // Main() 唯一性检查由 compile_module 在上层统一处理（已含 ProjectKind）,
    // 此处不再重复校验，避免库项目被错误拒绝。
    let is_windows = mangle::is_windows_target(target);
    let is_macos = matches!(target_os(target), TargetOs::Macos);
    // RFC 016 M4（用户裁决简化 2026-08-03）：相对 `library` 基准 = 执行程序根目录
    //（`-o` 输出可执行文件所在目录）。先统一解析为绝对路径，供符号验证 / 链接
    // 标志 / 运行时候选使用；`exe_dir` 同时供环境变量形式的相对路径运行期前置。
    let exe_dir = output.parent().unwrap_or_else(|| Path::new("."));
    let native_modules = &resolve_module_library_paths(native_modules, exe_dir);
    // RFC 016：计算生效加载策略（`load` 统一模型；`auto` 在此分流 static/runtime），
    // 须在符号验证、链接标志、IR 发射之前确定——单一事实来源。
    let effective_lib_paths = effective_native_lib_paths(native_modules, native_lib_paths, target);
    let strategies =
        native::verify_symbols::effective_load_strategies(native_modules, &effective_lib_paths);
    let runtime_native =
        native::build_runtime_infos(native_modules, &strategies, is_windows, is_macos, exe_dir);
    let native_symbols = native::build_native_symbol_table(native_modules);
    let native_callback_table = emit_native_callback::build_callback_table(native_modules);
    let native_link_libs = ensure_crypto_native_link_lib(
        ensure_wgpu_native_link_lib(
            native::native_link_libs(native_modules, &strategies),
            target,
        ),
        target,
    );
    let native_struct_types = native::emit_native_struct_types(native_modules);
    let mut ir_text;
    let sinit_diags;
    {
        let emitter = ModuleEmitter::new(
            layouts,
            is_windows,
            target.map(mangle::is_wasm_triple).unwrap_or(false),
            file_path,
            source,
            debug_info,
            fn_spans,
            &native_symbols,
            &native_callback_table,
            native_struct_types.clone(),
            generate_to_table,
            external_symbols,
            emit_role,
            None,
            &runtime_native,
        );
        let (ir, diags) = emitter.emit_module(fns);
        ir_text = ir;
        sinit_diags = diags;
    }

    // RFC 036 阶段 4·语义级裁剪闭环验证：编译期完整性门（arc-prune-001）。
    // 任一被引用符号既未定义也未声明 → 硬错误，拦截 reachability 过度裁剪。
    // **stub 补发闭环**：缺失符号若全部为 stub-handled（builtin 集合/门面的
    // 单态化实例，如 `HashSet_string_get_Item@hash`——其 MIR body 不存在，
    // tree-shake/force-keep 均无法从 mir_fns 保住），则将缺名以空 body 补入
    // fns 重发射——emit_fn 对 stub-handled 名走 `try_emit_stub` 生成 IR，
    // 不消费 MIR body。至多 4 轮（stub 引用其他 stub 的级联）；非 stub 缺失
    // 仍硬错误（原诊断语义）。
    let mut fns_vec: Vec<(String, mir::MirCfgBody)> = fns.to_vec();
    for _round in 0..4 {
        let missing = completeness::check_ir_complete_missing(&ir_text);
        if missing.is_empty() {
            break;
        }
        let mut patched = false;
        // 模板 body：stub 发射不消费 MIR body（try_emit_stub 提前返回），
        // 借用任一既有 stub-handled 条目的 body 克隆占位即可通过类型检查。
        let template_body = fns_vec
            .iter()
            .find(|(n, _)| crate::is_builtin_stub_fn(n))
            .map(|(_, b)| b.clone());
        for (sym, _line) in &missing {
            let bare = sym.split('@').next().unwrap_or(sym).to_string();
            if !crate::is_builtin_stub_fn(&bare) {
                continue;
            }
            if fns_vec.iter().any(|(name, _)| name == &bare) {
                continue;
            }
            let Some(mut body) = template_body.clone() else {
                continue;
            };
            body.linkage = mir::Linkage::LinkonceOdr;
            fns_vec.push((bare, body));
            patched = true;
        }
        if !patched {
            completeness::check_ir_complete(&ir_text)?;
            break;
        }
        let emitter = ModuleEmitter::new(
            layouts,
            is_windows,
            target.map(mangle::is_wasm_triple).unwrap_or(false),
            file_path,
            source,
            debug_info,
            fn_spans,
            &native_symbols,
            &native_callback_table,
            native_struct_types.clone(),
            generate_to_table,
            external_symbols,
            emit_role,
            None,
            &runtime_native,
        );
        let (ir, _) = emitter.emit_module(&fns_vec);
        ir_text = ir;
    }
    // 补发后的 fns 向后传递（debug 符号收集等）。
    let fns: &[(String, mir::MirCfgBody)] = &fns_vec;
    completeness::check_ir_complete(&ir_text)?;

    // RFC 015 Phase C: eliminate adjacent ARC retain/release pairs
    // within the same basic block before writing IR to disk.
    let _arc_elim = arc_optimize::eliminate_arc_pairs(&mut ir_text);

    // RFC 037 M2：仅当 IR 引用 UI/窗口 ABI 时才链接 crates/runtime-ui/platform/<os>/window.*。
    // 非 UI 可执行文件跳过，避免无谓依赖 d2d1/dwrite（Win32）。
    let needs_platform_window = ir_needs_platform_window(&ir_text);

    // Namespace intermediate products by the output file stem so parallel
    // builds (e.g. e2e tests sharing `target/e2e/`) don't clobber each
    // other's `out.ll` / `out.o` / `rt_*.o`. Each build gets its own subdir:
    //   target/e2e/object_model_test/out.ll
    //   target/e2e/collection_expr_test/out.ll
    //
    // obj_dir 优先（由 CLI 层保证绝对路径或源文件相对路径）；
    // 兜底使用 output 的父目录（如果 output 也没有父目录，用 "." 作为最后安全网）。
    let work_dir = {
        let stem = output.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        let base = obj_dir
            .map(|d| d.to_path_buf())
            .or_else(|| output.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(stem)
    };
    fs::create_dir_all(&work_dir)?;

    let ll_path = work_dir.join("out.ll");
    fs::write(&ll_path, &ir_text)?;

    // Compile runtime C sources → object files.
    // Each runtime concern lives in its own translation unit (rt_str, rt_dict,
    // rt_list, rt_array, rt_arc, rt_task, rt_exc) so they can be optimized and
    // audited independently. All sources share the same optimization level as
    // the generated IR so the runtime is not a hot-path bottleneck.
    //
    // Runtime .o cache: since runtime C sources are identical across all builds
    // with the same (target, opt_level, debug), we cache compiled .o files to
    // avoid redundant clang invocations. This is critical for parallel test
    // stability — without caching, N concurrent `arc build` processes would
    // each spawn ~11 clang processes, causing system resource exhaustion.
    let rt_base = crate::sdk_layout::sdk_runtime_base();
    let clang = clang_path();
    let level = if release {
        optimize::OptLevel::Release
    } else {
        optimize::OptLevel::Debug
    };

    // Compile runtime C sources → object files.
    let runtime_objs =
        prepare_runtime_objects(&rt_base, &clang, &work_dir, level, target, debug_info)?;

    // 用户「源实现」模块（`.ani` 内 `source` 声明）：把每个源实现 C 编译为 `.o`
    // 并链接。源实现模块的符号由本地 `.o` 提供 → 跳过外部 `-l<name>` 与外部库
    // 符号验证。DLL 回退发现（`.ani` 未声明 library 时同目录同名词库）已由加载器
    // 填入 `module.library`，经 `effective_native_lib_paths` 的 `-L` 纳入链接。
    let (user_native_objs, native_source_impl) =
        prepare_user_native_objects(native_modules, &clang, &work_dir, level, target, debug_info)?;

    // Compile out.ll → out.o (uses the same optimization level).
    let obj_path = work_dir.join("out.o");
    let status = optimize::clang_compile(&clang, &ll_path, &obj_path, level, target, debug_info)
        .status()
        .map_err(|e| CodegenError::Llvm(format!("clang not found: {e}")))?;
    if !status.success() {
        return Err(CodegenError::Llvm("clang IR compile failed".into()));
    }
    // RFC 017 产物域（U3 .ll 焚毁，UX 迭代评审 §2.3）：文本 IR 是用户目录中间
    // 产物膨胀的单项最大源（examples 实测 114 MB），clang 消费完即焚毁；
    // `--emit-llvm` 显式保留供 IR 诊断。clang 失败路径不焚毁（保留现场排障）。
    if !keep_ir {
        let _ = fs::remove_file(&ll_path);
    }

    // Link: out.o + runtime objects → executable
    // RFC 017 阶段一：runtime 层单副本共享——非 wasm 目标把 rt 核心 `.o` 从
    // 链接集剔除，改为导入引用共享 runtime（对标 C# coreclr.dll 进程单实例）；
    // wasm 无共享库形态（rt_wasm_min 内嵌路径），保持全量内嵌。
    let is_wasm = mangle::is_wasm_triple(target.unwrap_or(""));
    let shared_rt = if is_wasm {
        None
    } else {
        Some(shared_runtime::build_shared_runtime(
            &rt_base,
            &clang,
            &work_dir,
            target,
            level,
            debug_info,
        )?)
    };
    let shared_rt_input = shared_rt.as_ref().map(|a| a.link_input());
    let mut link_objs: Vec<&Path> = vec![&obj_path];
    for obj in &runtime_objs {
        let name = obj.file_name().and_then(|s| s.to_str()).unwrap_or("");
        // RFC 037 M1：`rt_wgpu_native.o`（wgpu ABI shim）不再无条件排除——
        // `WgpuRender` 属 Arc.UI 库，任何引用它的程序都需解析 wgpu shim 符号。
        // `-lwgpu_native` 已由 ensure_wgpu_native_link_lib 恒注入 + DLL 恒复制，
        // 故始终链接不引入额外依赖；非 UI 可执行文件同样安全（静态导入库无符号则
        // 不产生 wgpu_native.dll 运行时依赖）。
        if (is_platform_runtime_object(name) || is_ui_ime_runtime_object(name))
            && !needs_platform_window
        {
            continue;
        }
        // RFC 017 阶段一：rt 核心 `.o`（platform/ime/wgpu 之外的 runtime 单元）
        // 由共享 runtime 承载，exe 仅导入引用；platform/ime/wgpu 不在共享库内
        // （host 本地职责，与 build_shared_runtime 排除集一致），保留本地链接。
        if !is_wasm
            && !(is_platform_runtime_object(name)
                || is_ui_ime_runtime_object(name)
                || name == "rt_wgpu_native.o")
        {
            continue;
        }
        link_objs.push(obj.as_path());
    }
    // 用户源实现 `.o`：与 runtime 对象一同链接（源实现模块符号在此解析）。
    for obj in &user_native_objs {
        link_objs.push(obj.as_path());
    }
    // 共享 runtime 链接输入（导入库 / 共享库本体）置于对象集之后。
    if let Some(input) = &shared_rt_input {
        link_objs.push(input.as_path());
    }
    // RFC 016 M2: 编译期符号验证。在链接前对非 libc 的 native 模块执行符号存在性校验。
    // - 工具不可用 → 降级为 warning（RFC §9），不阻断编译
    // - 库路径无法定位 → 跳过该模块
    // - 符号缺失 → 返回 CodegenError，阻断编译
    // RFC 016：生效策略为 runtime 的模块（懒解析）自动跳过验证。
    // 用户源实现模块同样跳过——符号由本地 `.o` 提供（见 prepare_user_native_objects）。
    if let Err(detail) = native::verify_symbols::verify_all_native_modules(
        native_modules,
        &effective_lib_paths,
        &native_source_impl,
    ) {
        return Err(CodegenError::Llvm(format!(
            "native symbol verification failed: {detail}"
        )));
    }
    // RFC 016 M1/M2: 合并平台链接标志 + native 库搜索路径 + native 契约库标志。
    // 顺序：platform -l flags → -L<DIR>（lib_paths）→ -l<module>（native modules）。
    // -L 必须在 -l 之前，使链接器能在自定义路径找到契约库。
    let mut link_flags: Vec<String> = platform_link_flags(target)
        .iter()
        .map(|s| s.to_string())
        .collect();
    // RFC 017 阶段一：ELF/macOS 运行期以 rpath 定位产物同目录的共享库副本
    //（$ORIGIN / @executable_path）；Windows 同目录属默认搜索路径，无需标志。
    if shared_rt.is_some() {
        for flag in shared_runtime::consumer_rpath_flags(target) {
            link_flags.push(flag);
        }
    }
    // RFC 037 / 窗口子系统：UI 可执行文件链接为 Windows GUI 子系统以消除
    // 运行时控制台窗口（黑框）。真实入口仍是 `main`（见 gui_subsystem_flags）。
    if needs_platform_window {
        for flag in gui_subsystem_flags(target) {
            link_flags.push(flag);
        }
    }
    for path in &effective_lib_paths {
        link_flags.push(format!("-L{}", path.display()));
    }
    for lib in &native_link_libs {
        // 源实现模块不注入外部 `-l<name>`（符号由本地 `.o` 提供）。
        if native_source_impl.contains(lib.as_str()) {
            continue;
        }
        link_flags.push(format!("-l{lib}"));
    }
    let link_flags_refs: Vec<&str> = link_flags.iter().map(|s| s.as_str()).collect();
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CodegenError::Llvm(format!("create output dir failed: {e}")))?;
    }
    // DEBUG link (compile_via_llvm_ir path)
    eprintln!(
        "[link_debug2] needs_platform_window={} ir_has_wgpu={} total_objs={}",
        needs_platform_window,
        ir_text.contains("wgpu_"),
        link_objs.len()
    );
    for o in &link_objs {
        let n = o.file_name().and_then(|s| s.to_str()).unwrap_or("?");
        if n.contains("wgpu")
            || n.starts_with("platform_")
            || n == "out.o"
            || n.starts_with("rt_ui_")
        {
            eprintln!("  [link_debug2]  -> {}", o.display());
        }
    }
    eprintln!("  [link_debug2] native_link_libs={:?}", native_link_libs);
    let status = optimize::clang_link(&clang, &link_objs, output, target, level, &link_flags_refs)
        .status()
        .map_err(|e| CodegenError::Llvm(format!("link failed: {e}")))?;
    if !status.success() {
        return Err(CodegenError::Llvm("clang link failed".into()));
    }

    // RFC 017 阶段一：共享 runtime 落位产物同目录（缓存 + 硬链接单副本），
    // Windows 依赖解析与 ELF/macOS rpath 均以此为第一定位点。
    if let Some(artifact) = &shared_rt {
        shared_runtime::stage_shared_runtime(output, artifact);
    }

    // RFC 037 §D7.2: Windows 链接成功后复制 wgpu_native.dll 到输出目录。
    // 运行时 DLL 必须与 .exe 同目录才能被加载。
    copy_wgpu_native_dll_if_needed(output, target);
    // RFC 026 M1: 同理复制 vendored crypto_native.dll（`rt_crypto_*` ABI 底座）。
    copy_crypto_native_dll_if_needed(output, target);

    // RFC 017 M2: Generate .arcdbg debug symbol package (post-link).
    // Non-fatal: if .arcdbg generation fails, the binary still works,
    // just without symbolized backtraces.
    if debug_info {
        let line_starts = compute_line_starts(source);
        let symbols = collect_symbols(fns, file_path, fn_spans, &line_starts);
        let _ = crate::arcdbg::write_arcdbg(output, &symbols);
    }

    Ok(sinit_diags)
}

/// Compile MIR functions to a single object file (`.o`) — RFC 017 M3.
///
/// Unlike `compile_via_llvm_ir`, this **does not** link runtime or produce an
/// executable. It emits LLVM IR text, then invokes clang to compile it to a
/// relocatable object file. Used by `arc publish` to produce the `.o` payload
/// embedded in an `.ao` package.
///
/// `output` is the desired `.o` path. A `.ll` file is written for the clang
/// invocation and burned after success unless `keep_ir` is set (RFC 017:
/// `--emit-llvm`). No `main` function is required (library packages may omit it).
pub fn compile_to_object(
    fns: &[(String, MirCfgBody)],
    layouts: &ProgramLayouts,
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&str>,
    release: bool,
    file_path: &str,
    source: &str,
    debug_info: bool,
    fn_spans: &HashMap<String, Span>,
    native_modules: &[ast::NativeModule],
    generate_to_table: &GenerateToTable,
    external_symbols: &[typeck::ExternalSymbolEntry],
    emit_role: crate::EmitRole,
    package_meta: Option<crate::PackageMeta>,
    keep_ir: bool,
) -> Result<Vec<StaticInitDiagnostic>, CodegenError> {
    let is_windows = mangle::is_windows_target(target);
    let is_macos = matches!(target_os(target), TargetOs::Macos);
    // RFC 016 M4（用户裁决简化 2026-08-03）：相对 `library` 基准 = 执行程序根目录。
    // 发布路径无最终可执行位置，按 `.o` 输出目录烘焙（一致基准，确定性行为）。
    let exe_dir = output.parent().unwrap_or_else(|| Path::new("."));
    let native_modules = &resolve_module_library_paths(native_modules, exe_dir);
    // RFC 016：发布路径无 CLI lib_paths 上下文，auto 模块以空搜索列表分类
    //（有 `library` 目录的仍可定位 → static；否则降级 runtime）。懒解析器
    // 符号均为 `internal` linkage，与主程序各自持有副本，无跨 .o 冲突。
    let strategies = native::verify_symbols::effective_load_strategies(native_modules, &[]);
    let runtime_native =
        native::build_runtime_infos(native_modules, &strategies, is_windows, is_macos, exe_dir);
    let native_symbols = native::build_native_symbol_table(native_modules);
    let native_callback_table = emit_native_callback::build_callback_table(native_modules);
    let native_struct_types = native::emit_native_struct_types(native_modules);
    let emitter = ModuleEmitter::new(
        layouts,
        is_windows,
        target.map(mangle::is_wasm_triple).unwrap_or(false),
        file_path,
        source,
        debug_info,
        fn_spans,
        &native_symbols,
        &native_callback_table,
        native_struct_types,
        generate_to_table,
        external_symbols,
        emit_role,
        package_meta,
        &runtime_native,
    );
    let (mut ir_text, sinit_diags) = emitter.emit_module(fns);

    // RFC 036 阶段 4·语义级裁剪闭环验证：编译期完整性门（arc-prune-001）。
    completeness::check_ir_complete(&ir_text)?;

    // RFC 015 Phase C: eliminate adjacent ARC retain/release pairs.
    let _arc_elim = arc_optimize::eliminate_arc_pairs(&mut ir_text);

    let work_dir = {
        let stem = output.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        let base = obj_dir
            .map(|d| d.to_path_buf())
            .or_else(|| output.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(stem)
    };
    fs::create_dir_all(&work_dir)?;

    let ll_path = work_dir.join("out.ll");
    fs::write(&ll_path, &ir_text)?;

    let level = if release {
        optimize::OptLevel::Release
    } else {
        optimize::OptLevel::Debug
    };
    let clang = clang_path();
    let status = optimize::clang_compile(&clang, &ll_path, output, level, target, debug_info)
        .status()
        .map_err(|e| CodegenError::Llvm(format!("clang not found: {e}")))?;
    if !status.success() {
        return Err(CodegenError::Llvm("clang IR compile failed".into()));
    }
    // RFC 017 产物域（U3 .ll 焚毁）：与 compile_via_llvm_ir 同语义——clang 成功
    // 即焚毁文本 IR，`--emit-llvm` 显式保留；失败路径保留现场排障。
    if !keep_ir {
        let _ = fs::remove_file(&ll_path);
    }

    Ok(sinit_diags)
}

/// RFC 037 crates/runtime-ui/platform：中间产物 `platform_*.o`（common + 当前 OS 后端）。
fn is_platform_runtime_object(name: &str) -> bool {
    name.starts_with("platform_") && name.ends_with(".o")
}

fn is_ui_ime_runtime_object(name: &str) -> bool {
    name == "rt_ui_ime.o"
}

/// 按 target_os 选择 `crates/runtime-ui/platform/common/*` + 单一 OS 后端（禁止单体 #ifdef 堆叠）。
///
/// 架构红线（RFC 037 native-platform-layout）：UI 运行时归 `crates/runtime-ui/platform/`，
/// `crates/runtime/` 仅含非 UI 运行时。
fn platform_backend_sources(
    rt_base: &Path,
    work_dir: &Path,
    target: Option<&str>,
) -> Vec<(PathBuf, PathBuf)> {
    let plat = rt_base.join("runtime-ui/platform");
    if matches!(target_os(target), TargetOs::Host) {
        return platform_backend_sources(
            rt_base,
            work_dir,
            if cfg!(windows) {
                Some("x86_64-pc-windows-msvc")
            } else if cfg!(target_os = "linux") {
                Some("x86_64-unknown-linux-gnu")
            } else if cfg!(target_os = "macos") {
                Some("aarch64-apple-darwin")
            } else {
                None
            },
        );
    }
    let mut out = vec![
        (
            plat.join("common/rt_ui_element.c"),
            work_dir.join("platform_common_element.o"),
        ),
        (
            plat.join("common/rt_ui_window_bridge.c"),
            work_dir.join("platform_common_bridge.o"),
        ),
        (
            plat.join("common/rt_ui_pointer.c"),
            work_dir.join("platform_common_pointer.o"),
        ),
        (
            plat.join("common/rt_ui_props.c"),
            work_dir.join("platform_common_props.o"),
        ),
        (
            plat.join("common/rt_ui_scroll_dispatch.c"),
            work_dir.join("platform_common_scroll_dispatch.o"),
        ),
        (
            plat.join("common/rt_ui_image_common.c"),
            work_dir.join("platform_common_image_common.o"),
        ),
        (
            plat.join("common/rt_ui_keyboard.c"),
            work_dir.join("platform_common_keyboard.o"),
        ),
        (
            plat.join("common/rt_ui_ime_bridge.c"),
            work_dir.join("platform_common_ime_bridge.o"),
        ),
    ];
    match target_os(target) {
        TargetOs::Windows => {
            out.push((
                plat.join("windows/window.cpp"),
                work_dir.join("platform_windows.o"),
            ));
            out.push((
                plat.join("windows/ime_platform.c"),
                work_dir.join("platform_windows_ime.o"),
            ));
            out.push((
                plat.join("windows/pointer_win32.c"),
                work_dir.join("platform_windows_pointer.o"),
            ));
            out.push((
                plat.join("windows/scroll_win32.c"),
                work_dir.join("platform_windows_scroll.o"),
            ));
            out.push((
                plat.join("windows/rt_ui_image_win32.cpp"),
                work_dir.join("platform_windows_image_load.o"),
            ));
            out.push((
                plat.join("windows/ime_win32.c"),
                work_dir.join("platform_windows_ime_dispatch.o"),
            ));
            out.push((
                plat.join("windows/keyboard_win32.c"),
                work_dir.join("platform_windows_keyboard.o"),
            ));
            out.push((
                plat.join("windows/rt_ui_scrollbar.cpp"),
                work_dir.join("platform_windows_scrollbar.o"),
            ));
        }
        TargetOs::Linux => {
            out.push((
                plat.join("linux/window.cpp"),
                work_dir.join("platform_linux.o"),
            ));
            out.push((
                plat.join("linux/ime_platform.c"),
                work_dir.join("platform_linux_ime.o"),
            ));
            out.push((
                plat.join("common/rt_ui_image_stub.c"),
                work_dir.join("platform_linux_image_stub.o"),
            ));
            out.push((
                plat.join("common/rt_ui_scrollbar_stub.c"),
                work_dir.join("platform_linux_scrollbar_stub.o"),
            ));
        }
        TargetOs::Macos => {
            out.push((
                plat.join("macos/window.mm"),
                work_dir.join("platform_macos.o"),
            ));
            out.push((
                plat.join("macos/ime_platform.c"),
                work_dir.join("platform_macos_ime.o"),
            ));
            out.push((
                plat.join("common/rt_ui_scrollbar_stub.c"),
                work_dir.join("platform_macos_scrollbar_stub.o"),
            ));
        }
        TargetOs::Ohos => {
            out.push((
                plat.join("ohos/window_stub.c"),
                work_dir.join("platform_ohos.o"),
            ));
            out.push((
                plat.join("ohos/ime_platform.c"),
                work_dir.join("platform_ohos_ime.o"),
            ));
            out.push((
                plat.join("common/rt_ui_scrollbar_stub.c"),
                work_dir.join("platform_ohos_scrollbar_stub.o"),
            ));
        }
        TargetOs::WebAssembly | TargetOs::Wasi => {}
        TargetOs::Host => unreachable!("handled above"),
    }
    out
}

/// 发现层自动化（原生 ABI 扩展路径）：扫描 `crates/runtime/` 下全部 `rt_*.c`
/// 独立 TU，编译期自动纳入链接——新增原生 ABI 源文件无需改编译器。
/// 命名即契约：`rt_` 前缀 + `.c` 后缀；`rt_wasm_min.c` 仅供 wasm 目标
/// （上方 wasm 分支单独引用），在此剔除。排序保证确定性（依赖树哈希与
/// 内容寻址缓存按「文件集合」而非顺序判定，见 `prepare_runtime_objects`
/// 的 `deps_hash`）。
fn runtime_rt_sources(
    rt_dir: &Path,
    work_dir: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, CodegenError> {
    let mut names: Vec<String> = std::fs::read_dir(rt_dir)
        .map_err(|e| CodegenError::Llvm(format!("scan runtime dir failed: {e}")))?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.ends_with(".c") && name.starts_with("rt_") && name != "rt_wasm_min.c")
                .then_some(name)
        })
        .collect();
    names.sort();
    Ok(names
        .into_iter()
        .map(|name| (rt_dir.join(&name), work_dir.join(name.replace(".c", ".o"))))
        .collect())
}

/// 收集用户「源实现」模块的 C 源（由 `.ani` 内 `source` 声明，基准 = 契约目录）。
///
/// 对齐 DLL 显式声明模型：`library` 声明库路径、`source` 声明源码路径——编译器
/// 只认 `.ani` 契约，C 源路径由契约自声明（用户无需改动编译器）。仅当模块声明
/// 了 `source` 才纳入源实现：其符号由本地编译的 `.o` 提供，跳过外部 `-l<name>`
/// 与外部库符号验证；未声明 `source` 维持原设计（经 `library`/搜索列表链接）。
///
/// 返回 `(c_source, obj)` 列表（obj 落在 work_dir，按模块名排序保证确定性）。
fn source_impl_sources(
    modules: &[ast::NativeModule],
    work_dir: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, CodegenError> {
    let mut entries: Vec<(String, PathBuf, PathBuf)> = modules
        .iter()
        .filter_map(|m| {
            let src = m.source.clone()?;
            let obj = work_dir.join(format!("{}.o", m.name.as_str()));
            Some((m.name.to_string(), src, obj))
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries
        .into_iter()
        .map(|(_, src, obj)| (src, obj))
        .collect())
}

/// 编译用户 native 源实现 → 对象文件，并返回源实现模块名集合。
///
/// 与 [`runtime_rt_sources`]/[`prepare_runtime_objects`] 对称：把用户 `.c` 当作
/// 一等编译输入，编译出的 `.o` 进入链接，源实现模块被排除在外部静态链接之外。
/// 注入 `-I<c_source 父目录>`，使 `.c` 可 `#include` 同目录头文件（自包含源实现）。
fn prepare_user_native_objects(
    modules: &[ast::NativeModule],
    clang: &str,
    work_dir: &Path,
    level: optimize::OptLevel,
    target: Option<&str>,
    debug_info: bool,
) -> Result<(Vec<PathBuf>, std::collections::HashSet<String>), CodegenError> {
    let sources = source_impl_sources(modules, work_dir)?;
    let mut objs = Vec::with_capacity(sources.len());
    let mut names = std::collections::HashSet::with_capacity(sources.len());
    for (src, obj) in &sources {
        let include_dir = src.parent().unwrap_or_else(|| Path::new("."));
        let status = optimize::clang_compile(clang, src, obj, level, target, debug_info)
            .arg(format!("-I{}", include_dir.display()))
            .status()
            .map_err(|e| CodegenError::Llvm(format!("clang not found: {e}")))?;
        if !status.success() {
            return Err(CodegenError::Llvm(format!(
                "user native C compile failed: {}",
                src.display()
            )));
        }
        names.insert(
            src.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
        );
        objs.push(obj.clone());
    }
    Ok((objs, names))
}

/// Compile runtime C sources → object files (RFC 017 M4-link 抽取).
///
/// 共享 runtime object 缓存（按 target/opt_level/debug 键），避免并发构建
/// 重复编译同一份 runtime 源码。`compile_via_llvm_ir`（编译 + 链接）与
/// `link_objects_to_executable`（纯链接）共用此函数。
fn prepare_runtime_objects(
    rt_base: &Path,
    clang: &str,
    work_dir: &Path,
    level: optimize::OptLevel,
    target: Option<&str>,
    debug_info: bool,
) -> Result<Vec<PathBuf>, CodegenError> {
    let rt_dir = rt_base.join("runtime");
    let rt_sources: Vec<(PathBuf, PathBuf)> = if target.map(mangle::is_wasm_triple).unwrap_or(false)
    {
        vec![(rt_dir.join("rt_wasm_min.c"), work_dir.join("rt_wasm_min.o"))]
    } else {
        let platform_sources = platform_backend_sources(rt_base, work_dir, target);
        let sqlite3_c = rt_base.join("runtime-sqlite/sqlite3.c");
        let rt_image_c = rt_base.join("runtime-drawing/rt_image.c"); // RFC 029 M1
                                                                     // RFC 029 M2/M4/M6：qrcodegen（独立 TU，rt_qrcode.c 引用）· quirc 单 TU
                                                                     // 合并（rt_barcode.c 内 #include quirc.c/decode.c/identify.c/version_db.c）·
                                                                     // stb_truetype 单 TU（rt_font.c 内 STB_TRUETYPE_IMPLEMENTATION）。
        let rt_qrcode_c = rt_base.join("runtime-drawing/rt_qrcode.c"); // RFC 029 M2
        let qrcodegen_c = rt_base.join("runtime-drawing/qrcodegen.c"); // RFC 029 M2（独立 TU）
        let rt_barcode_c = rt_base.join("runtime-drawing/rt_barcode.c"); // RFC 029 M4
        let rt_font_c = rt_base.join("runtime-drawing/rt_font.c"); // RFC 029 M6
        runtime_rt_sources(&rt_dir, work_dir)?
            .into_iter()
            .chain(platform_sources)
            // 架构红线（RFC 037 native-platform-layout）：UI 运行时归 `crates/runtime-ui/`。
            // rt_wgpu_native.c / rt_editor.c / rt_ui_ime.c 位于 runtime-ui 根目录，独立于
            // 非 UI 运行时（crates/runtime）与平台后端（runtime-ui/platform）。rt_wgpu_native.c
            // 命中下方循环内 vendoring `-I` 注入（按文件名 rt_wgpu_native.c 匹配）。
            .chain({
                let ui_dir = rt_base.join("runtime-ui");
                [
                    ("rt_wgpu_native.c", "rt_wgpu_native.o"),
                    ("rt_editor.c", "rt_editor.o"),
                    ("rt_ui_ime.c", "rt_ui_ime.o"),
                ]
                .iter()
                .map(|(name, obj)| {
                    let src = ui_dir.join(name);
                    let obj = work_dir.join(obj);
                    (src, obj)
                })
                .collect::<Vec<_>>()
            })
            .chain(std::iter::once((sqlite3_c, work_dir.join("sqlite3.o"))))
            .chain(std::iter::once((rt_image_c, work_dir.join("rt_image.o"))))
            .chain(std::iter::once((rt_qrcode_c, work_dir.join("rt_qrcode.o"))))
            .chain(std::iter::once((qrcodegen_c, work_dir.join("qrcodegen.o"))))
            .chain(std::iter::once((
                rt_barcode_c,
                work_dir.join("rt_barcode.o"),
            )))
            .chain(std::iter::once((rt_font_c, work_dir.join("rt_font.o"))))
            .collect()
    };

    // Shared cache directory keyed by (target, opt_level, debug_flag, sanitize).
    // RFC 005 里程碑②（always-on）：`-DARC_CYCLE_COLLECTION` **无条件恒注入**（见下），
    // 收集器恒编译进每个二进制 → rt_arc.o 恒为「带收集器」**单一变体**。⑤ 已移除
    // 用户开关并收敛缓存键为无后缀单一路径（`_cc` 后缀分支删除）。
    // sanitize 后缀：`sanitize_flag()` 注入的 `-fsanitize=` 会改变产物（插桩 runtime
    // `.o` 引用 `__asan_*` 符号），必须纳入缓存键，否则 ASan 产物与非 ASan 产物互串
    // 复用 → 链接缺 ASan 运行时符号失败（`__asan_init` 等）或反之残留插桩。
    let sanitize_suffix = optimize::sanitize_flag()
        .map(|f| f.replace("=", "_"))
        .map(|f| format!("_{f}"))
        .unwrap_or_default();
    let rt_cache_subdir = crate::sdk_layout::runtime_cache_dir().join(format!(
        "{}_{}_{}{}",
        target.unwrap_or("default"),
        if matches!(level, optimize::OptLevel::Release) {
            "release"
        } else {
            "debug"
        },
        if debug_info { "g" } else { "nog" },
        sanitize_suffix
    ));

    // ── 内容寻址缓存（rt_cache 模块）：mtime 完全退出命中判定 ──
    // 历史教训（base64_bytes 链接失败）：mtime 比较会复用陈旧 `.o`，且头文件
    // 与被 `#include` 的 `.c` 变化不感知。现按内容哈希寻址：
    //   - src_hash：单源内容（独立 TU 的 `.c` 变化只重编自身）；
    //   - deps_hash：全局依赖树（全部头文件 + 非独立 TU 的 `.c`）——任何
    //     头文件/被 include 源变化触发全量重编；
    //   - flags_hash：编译选项（target/opt/debug/sanitize/恒定注入/`-I` 注入）
    //     + clang 版本（升级改变代码生成 → 旧缓存失效）。
    // 内容未变（含 touch）必命中；任一输入变化必重编；产物损坏（size 不符）
    // 自愈重编。
    let independent_sources: Vec<PathBuf> = rt_sources.iter().map(|(s, _)| s.clone()).collect();
    let deps_hash = crate::rt_cache::compute_deps_hash(rt_base, &independent_sources);
    // 编译选项的稳定分量（与下方编译分支的 `-I`/`-D` 注入一一对应；新增注入
    // 必须同步此处——见 rt_cache::flags_hash 维护规则）。
    let mut flags_extra: Vec<&str> = vec!["cc"]; // `-DARC_CYCLE_COLLECTION` 恒注入
    if rt_base.join("runtime-ui/wgpu-native/include").exists() {
        flags_extra.push("iwgpu");
    }
    if rt_base.join("runtime-ui/platform/common").exists() {
        flags_extra.push("iplat");
    }
    if rt_base.join("runtime-sqlite").exists() {
        flags_extra.push("isqlite");
    }
    if rt_base.join("runtime-drawing").exists() {
        flags_extra.push("idrawing");
    }
    let mut flags_hash = crate::rt_cache::flags_hash(
        target,
        matches!(level, optimize::OptLevel::Release),
        debug_info,
        &sanitize_suffix,
        &flags_extra,
    );
    flags_hash.push('|');
    flags_hash.push_str(&crate::rt_cache::clang_version_fingerprint(clang));

    let mut runtime_objs: Vec<PathBuf> = Vec::new();
    for (src, obj) in &rt_sources {
        let cached_obj = rt_cache_subdir.join(obj.file_name().unwrap());
        let src_hash = crate::rt_cache::file_sha256(src);
        // deps 树不可读（异常态）→ 不寻址、不回填，每次保守重编。
        let cacheable = deps_hash.is_some() && src_hash.is_some();
        let want = crate::rt_cache::CacheFingerprint {
            src_hash: src_hash.clone().unwrap_or_default(),
            deps_hash: deps_hash.clone().unwrap_or_default(),
            flags_hash: flags_hash.clone(),
            size: 0, // 命中判定以指纹记录值对比实际产物（见 cache_hit）。
            obj_hash: String::new(),
        };
        let use_cached = cacheable && crate::rt_cache::cache_hit(&cached_obj, &want);
        match std::env::var("ARC_RT_CACHE_VERBOSE").as_deref() {
            Ok("1") => eprintln!(
                "[rt_cache] {} -> {}",
                src.display(),
                if use_cached { "hit" } else { "miss" }
            ),
            Ok("2") => eprintln!(
                "[rt_cache] {} -> {} (src={} deps={} flags={})",
                src.display(),
                if use_cached { "hit" } else { "miss" },
                &want.src_hash[..8],
                &want.deps_hash[..8],
                &want.flags_hash[..8],
            ),
            _ => {}
        }

        if use_cached {
            runtime_objs.push(cached_obj);
            continue;
        }

        // ── 缓存自净（L3，根治「陈旧/损坏缓存反复致失败」）──
        // `use_cached == false` 的常见根因是缓存条目**陈旧/损坏/无指纹**
        // （src/deps/flags 变化、或上个断构建/沙箱写入留下的半写残骸）。
        // 这类条目已不会被本构建复用（cache_hit 返 miss → 下面重编全新
        // work_dir 对象），但若不主动清除，它会**永久滞留在缓存目录**，
        // 反复触发「看似随机」的过程启动 0xC0000005，被误判为 codegen /
        // QIF 源码缺陷——历史教训即此。此处一旦判定该条目不可用即连同
        // 指纹一起删除（自愈自净），把「手工 `rm -rf <cache>/<triple>`」
        // 的排查循环变成构建无条件自净；删除时机安全：进程级独占本构建
        // 的 work_dir 对象，cond 重编后回填，绝不丢失可复用产物。
        if cached_obj.exists() || crate::rt_cache::meta_path(&cached_obj).exists() {
            let purged = std::fs::remove_file(&cached_obj).is_ok();
            let _ = std::fs::remove_file(crate::rt_cache::meta_path(&cached_obj));
            if matches!(
                std::env::var("ARC_RT_CACHE_VERBOSE").as_deref(),
                Ok("1" | "2")
            ) {
                eprintln!(
                    "[rt_cache] purged stale/corrupt cache entry {}{} -> recompiling fresh {}",
                    cached_obj.display(),
                    if purged { "" } else { " (obj already absent)" },
                    src.display(),
                );
            }
        }

        // Compile to work_dir (per-build, avoids concurrent write conflicts).
        let mut cmd = optimize::clang_compile(clang, src, obj, level, target, debug_info);
        // RFC 005 里程碑②（always-on）：`-DARC_CYCLE_COLLECTION` **无条件恒注入**
        // ——`g_cycle_collection_enabled` 在每二进制内默认 1（收集器编译进每个
        // 二进制、默认开启、用户无感；RFC 005 §0/§2.1）。仅在该源上添加，避免
        // 污染其他 runtime 源。⑤ 已物理移除用户开关（RFC 005 ⑤）。
        if src.file_name().and_then(|s| s.to_str()) == Some("rt_arc.c") {
            cmd.arg("-DARC_CYCLE_COLLECTION");
        }
        // RFC 037 §D7.2：rt_wgpu_native.c 引用 <webgpu.h> / <wgpu.h>，
        // 注入 vendoring 头文件搜索路径 `crates/runtime-ui/wgpu-native/include`。
        // 仅在该源上添加，避免污染其他 runtime 源的搜索路径。
        if src.file_name().and_then(|s| s.to_str()) == Some("rt_wgpu_native.c") {
            let wgpu_inc = rt_base.join("runtime-ui/wgpu-native/include");
            if wgpu_inc.exists() {
                cmd.arg(format!("-I{}", wgpu_inc.display()));
            }
        }
        if is_platform_runtime_object(obj.file_name().and_then(|s| s.to_str()).unwrap_or("")) {
            let plat_common = rt_base.join("runtime-ui/platform/common");
            cmd.arg(format!("-I{}", plat_common.display()));
        }
        // L3 Orm：rt_sqlite.c / sqlite3.c 共用 amalgamation 头路径
        let src_name = src.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if src_name == "rt_sqlite.c" || src_name == "sqlite3.c" {
            let sqlite_inc = rt_base.join("runtime-sqlite");
            if sqlite_inc.exists() {
                cmd.arg(format!("-I{}", sqlite_inc.display()));
            }
        }
        // RFC 029 M1：rt_image.c 引用同目录 vendored stb 头（stb_image.h /
        // stb_image_write.h），注入 vendoring 头文件搜索路径 `crates/runtime-drawing`。
        // RFC 029 M2/M4/M6 同理：rt_qrcode.c/qrcodegen.c 引用 qrcodegen.h、
        // rt_barcode.c 引用 quirc.h（并 #include quirc.c 等）、rt_font.c 引用
        // stb_truetype.h——统一注入 `crates/runtime-drawing`（对齐 sqlite 注入先例）。
        if src_name == "rt_image.c"
            || src_name == "rt_qrcode.c"
            || src_name == "qrcodegen.c"
            || src_name == "rt_barcode.c"
            || src_name == "rt_font.c"
        {
            let drawing_inc = rt_base.join("runtime-drawing");
            if drawing_inc.exists() {
                cmd.arg(format!("-I{}", drawing_inc.display()));
            }
        }
        if src.extension().and_then(|s| s.to_str()) == Some("mm") {
            cmd.arg("-fobjc-arc");
        }
        let status = cmd
            .status()
            .map_err(|e| CodegenError::Llvm(format!("clang not found: {e}")))?;
        if !status.success() {
            return Err(CodegenError::Llvm(format!(
                "compile failed: {}",
                src.display()
            )));
        }

        // Best-effort: copy to cache for future reuse by other builds.
        // If this fails (concurrent access, permissions), the build still works.
        //
        // RFC 036 M5：缓存回填必须是**原子写**（temp + rename）。`cargo test -p
        // arc-integration` 会并行拉起多个 `arc build`，它们同时向共享
        // `target/rt_cache/` 写同一 `.o`；若直接 `fs::copy`，读取方（链接器）可能在
        // 写入方写到一半时按 `metadata().modified()` 判为新 → 链接**截断 .o** →
        // 产物内固化损坏代码 → 运行时栈破坏（`STATUS_STACK_BUFFER_OVERRUN`）且
        // **逐二进制确定复现**（同源码重建即恢复）——zxing_unavailable/zv2 间歇
        // 崩溃即此。temp 与目标同目录同卷，`fs::rename` 为原子替换（Windows
        // MoveFileExW MOVEFILE_REPLACE_EXISTING）：读者只会看到完整旧文件或完整
        // 新文件，绝不半写。临时文件残留无副作用（下次 copy 覆盖）。
        //
        // 内容寻址：`.o` 回填后**先落指纹文件**（同为 temp + rename 原子写）——
        // 若进程在两步间崩溃，下次判定 src/deps/flags 或 size 不匹配 → 重编
        // （自愈，绝不复用半写产物）。
        if cacheable {
            let _ = fs::create_dir_all(&rt_cache_subdir);
            let tmp_obj = cached_obj.with_extension("tmp");
            if fs::copy(obj, &tmp_obj).is_ok() {
                let _ = fs::rename(&tmp_obj, &cached_obj);
            }
            let recorded = crate::rt_cache::CacheFingerprint {
                size: fs::metadata(obj).map(|m| m.len()).unwrap_or(0),
                obj_hash: crate::rt_cache::file_sha256(obj).unwrap_or_default(),
                ..want
            };
            crate::rt_cache::write_meta_atomic(&cached_obj, &recorded);
        }

        runtime_objs.push(obj.clone());
    }

    Ok(runtime_objs)
}

/// RFC 037 M2：检测 LLVM IR 是否引用 crates/runtime-ui/platform 窗口/UI ABI 符号。
fn ir_needs_platform_window(ir: &str) -> bool {
    ir.contains("@rt_ui_") || ir.contains("@__arc_window_") || ir.contains("@rt_window_")
}

/// RFC 037 M1 修复：检测预编译 `.o` 文件是否引用 UI/WGPU ABI 符号。
///
/// 与 `ir_needs_platform_window` 对偶——`compile_via_llvm_ir` 在 IR 文本上
/// 做子串匹配；`link_objects_to_executable` 接收预编译 `.o`（无 IR 文本），
/// 这里对每个 `.o` 的原始字节做 ASCII 子串扫描，检测 UI/WGPU 符号引用。
///
/// 命中 `rt_ui_` / `rt_window_` / `__arc_window_` / `wgpu_` 任一子串即判定
/// 需要链接 platform/ime/wgpu runtime 对象。COFF/ELF 符号表以原始字节存储
/// 符号名，子串匹配零误报（这些前缀唯一绑定到 UI/WGPU ABI）。
fn objs_need_platform_window(objs: &[PathBuf]) -> bool {
    /// 零分配字节子串搜索（b"prefix" 在 data 中出现即返回 true）。
    fn contains_bytes(data: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() || data.len() < needle.len() {
            return false;
        }
        data.windows(needle.len()).any(|w| w == needle)
    }
    let needles: &[&[u8]] = &[b"rt_ui_", b"rt_window_", b"__arc_window_", b"wgpu_"];
    for obj in objs {
        let Ok(data) = fs::read(obj) else { continue };
        for needle in needles {
            if contains_bytes(&data, needle) {
                return true;
            }
        }
    }
    false
}

/// Link pre-compiled object files + runtime + native libraries into a single
/// executable (RFC 017 M4-link 子能力).
///
/// 与 `compile_via_llvm_ir` 解耦的纯链接入口——供 `arc test` 等"先各自编译
/// 为 `.o` 再链接"场景使用（如 QIF harness 链接被测 `.ao` 提取的 `.o` +
/// 测试源码 `.o` + runtime → 单一测试二进制）。
///
/// 不要求 `objs` 中存在 `main` 函数（调用方负责确保链接产物有入口点）。
/// runtime `.o` 由 `prepare_runtime_objects` 编译缓存。
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
    let rt_base = crate::sdk_layout::sdk_runtime_base();
    let clang = clang_path();
    let level = if release {
        optimize::OptLevel::Release
    } else {
        optimize::OptLevel::Debug
    };

    let work_dir = {
        let stem = output.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        let base = obj_dir
            .map(|d| d.to_path_buf())
            .or_else(|| output.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(stem)
    };
    fs::create_dir_all(&work_dir)?;

    // RFC 017 阶段一：runtime 层单副本共享——非 wasm 目标构建/复用共享 runtime，
    // rt 核心 `.o` 从链接集剔除改为导入引用；wasm 保持内嵌（无共享库形态）。
    let is_wasm = mangle::is_wasm_triple(target.unwrap_or(""));
    let shared_rt = if is_wasm {
        None
    } else {
        Some(shared_runtime::build_shared_runtime(
            &rt_base,
            &clang,
            &work_dir,
            target,
            level,
            debug_info,
        )?)
    };
    let shared_rt_input = shared_rt.as_ref().map(|a| a.link_input());

    let runtime_objs =
        prepare_runtime_objects(&rt_base, &clang, &work_dir, level, target, debug_info)?;

    // 用户「源实现」模块（`.ani` 内 `source` 声明）：把每个源实现 C 编译为 `.o`
    // 并链接。源实现模块的符号由本地 `.o` 提供 → 跳过外部 `-l<name>` 与外部库
    // 符号验证。DLL 回退发现（`.ani` 未声明 library 时同目录同名词库）已由加载器
    // 填入 `module.library`，经 `effective_native_lib_paths` 的 `-L` 纳入链接。
    let (user_native_objs, native_source_impl) =
        prepare_user_native_objects(native_modules, &clang, &work_dir, level, target, debug_info)?;

    // RFC 016 M4（用户裁决简化 2026-08-03）：相对 `library` 基准 = 执行程序根目录。
    // 链接阶段同样按 `-o` 输出目录解析，与 IR 发射阶段一致（否则 -L 标志 / 符号
    // 验证会在不同基准下查找库文件）。
    let exe_dir = output.parent().unwrap_or_else(|| Path::new("."));
    let native_modules = &resolve_module_library_paths(native_modules, exe_dir);

    // RFC 016 M2 + RFC 037 §D7.2: 计算有效 lib paths，自动注入 wgpu-native
    // vendor lib 目录。该 Vec 同时用于符号验证与链接器 -L 标志。
    let effective_lib_paths = effective_native_lib_paths(native_modules, native_lib_paths, target);

    // RFC 016：生效策略（auto 分流）——runtime 模块跳过符号验证与静态链接。
    let strategies =
        native::verify_symbols::effective_load_strategies(native_modules, &effective_lib_paths);

    // RFC 016 M2: 编译期符号验证（与 compile_via_llvm_ir 一致）。
    if let Err(detail) = native::verify_symbols::verify_all_native_modules(
        native_modules,
        &effective_lib_paths,
        &native_source_impl,
    ) {
        return Err(CodegenError::Llvm(format!(
            "native symbol verification failed: {detail}"
        )));
    }

    let native_link_libs = ensure_crypto_native_link_lib(
        ensure_wgpu_native_link_lib(
            native::native_link_libs(native_modules, &strategies),
            target,
        ),
        target,
    );
    let mut link_flags: Vec<String> = mangle::platform_link_flags(target)
        .iter()
        .map(|s| s.to_string())
        .collect();
    // RFC 017 阶段一：ELF/macOS 运行期以 rpath 定位产物同目录的共享库副本。
    if shared_rt.is_some() {
        for flag in shared_runtime::consumer_rpath_flags(target) {
            link_flags.push(flag);
        }
    }

    // RFC 037 M1 修复：检测输入 `.o` 是否引用 UI/WGPU ABI 符号，决定是否
    // 链接 platform/ime/wgpu runtime 对象与 GUI 子系统标志（与 compile_via_llvm_ir
    // 路径的 `ir_needs_platform_window` / `needs_platform_window` 对偶）。
    let needs_platform_window = objs_need_platform_window(objs);
    if needs_platform_window {
        for flag in gui_subsystem_flags(target) {
            link_flags.push(flag.to_string());
        }
    }
    for path in &effective_lib_paths {
        link_flags.push(format!("-L{}", path.display()));
    }
    for lib in &native_link_libs {
        // 源实现模块不注入外部 `-l<name>`（符号由本地 `.o` 提供）。
        if native_source_impl.contains(lib.as_str()) {
            continue;
        }
        link_flags.push(format!("-l{lib}"));
    }
    let link_flags_refs: Vec<&str> = link_flags.iter().map(|s| s.as_str()).collect();

    let mut link_objs: Vec<&Path> = Vec::with_capacity(objs.len() + runtime_objs.len());
    for o in objs {
        link_objs.push(o);
    }
    for obj in &runtime_objs {
        let name = obj.file_name().and_then(|s| s.to_str()).unwrap_or("");
        // RFC 037 M1：`rt_wgpu_native.o`（wgpu ABI shim）始终链接——
        // `WgpuRender` 属 Arc.UI 库，任何引用它的程序都需解析 wgpu shim 符号；
        // 非 UI 程序经 --gc-sections 回收无引用段，不产生 DLL 加载依赖。
        // platform_* / rt_ui_ime：仅当输入 `.o` 引用 UI ABI 时链接，
        // 与 compile_via_llvm_ir 的 `!needs_platform_window → skip` 对偶。
        if (is_platform_runtime_object(name) || is_ui_ime_runtime_object(name))
            && !needs_platform_window
        {
            continue;
        }
        // RFC 017 阶段一：rt 核心 `.o` 由共享 runtime 承载，exe 仅导入引用；
        // platform/ime/wgpu 保留本地链接（host 本地职责）。
        if !is_wasm
            && !(is_platform_runtime_object(name)
                || is_ui_ime_runtime_object(name)
                || name == "rt_wgpu_native.o")
        {
            continue;
        }
        link_objs.push(obj.as_path());
    }
    // 用户源实现 `.o`：与 runtime 对象一同链接（源实现模块符号在此解析）。
    for obj in &user_native_objs {
        link_objs.push(obj.as_path());
    }
    // 共享 runtime 链接输入（导入库 / 共享库本体）置于对象集之后。
    if let Some(input) = &shared_rt_input {
        link_objs.push(input.as_path());
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CodegenError::Llvm(format!("create output dir failed: {e}")))?;
    }
    // DEBUG link
    eprintln!(
        "[link_debug] needs_platform_window={} total_objs={}",
        needs_platform_window,
        link_objs.len()
    );
    for o in &link_objs {
        let n = o.file_name().and_then(|s| s.to_str()).unwrap_or("?");
        if n.contains("wgpu")
            || n.starts_with("platform_")
            || n == "out.o"
            || n.starts_with("rt_ui_")
        {
            eprintln!("  [link_debug]   -> {}", o.display());
        }
    }
    let status = optimize::clang_link(&clang, &link_objs, output, target, level, &link_flags_refs)
        .status()
        .map_err(|e| CodegenError::Llvm(format!("link failed: {e}")))?;
    if !status.success() {
        return Err(CodegenError::Llvm("clang link failed".into()));
    }

    // RFC 017 阶段一：共享 runtime 落位产物同目录（缓存 + 硬链接单副本）。
    if let Some(artifact) = &shared_rt {
        shared_runtime::stage_shared_runtime(output, artifact);
    }

    // RFC 037 §D7.2: Windows 链接成功后复制 wgpu_native.dll 到输出目录。
    copy_wgpu_native_dll_if_needed(output, target);
    // RFC 026 M1: 同理复制 vendored crypto_native.dll（`rt_crypto_*` ABI 底座）。
    copy_crypto_native_dll_if_needed(output, target);

    Ok(())
}

/// Link pre-compiled object files + runtime + native libraries into a shared
/// dynamic library — RFC 017 D8 v1.0.
///
/// 与 [`link_objects_to_executable`] 平行的纯链接入口，产物为动态库
/// （Windows `.dll` / Linux `.so` / macOS `.dylib`）。
///
/// # 与可执行文件链接的差异
///
/// - 使用 `clang_link_shared`（`-shared` + `-fPIC`）而非 `clang_link`
/// - 不应用 section GC：动态库的所有导出符号必须保留
/// - `export_symbols` 列出领域约定符号（如 `__qif_init`），Windows MSVC
///   下转换为 `/EXPORT:<symbol>` 标志显式导出
///
/// # 典型流程
///
/// 1. `arc build --kind library --dynamic` 将项目源码编译为 `.o`
/// 2. 调用本函数链接 `.o` + runtime → 单一动态库
/// 3. host 程序通过 `rt_library_load` + `rt_library_sym` 加载并查找领域约定符号
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
    let rt_base = crate::sdk_layout::sdk_runtime_base();
    let clang = clang_path();
    let level = if release {
        optimize::OptLevel::Release
    } else {
        optimize::OptLevel::Debug
    };

    let work_dir = {
        let stem = output.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        let base = obj_dir
            .map(|d| d.to_path_buf())
            .or_else(|| output.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(stem)
    };
    fs::create_dir_all(&work_dir)?;

    // RFC 017 阶段一：runtime 层单副本共享——非 wasm 目标构建/复用共享 runtime，
    // 插件 dll 不再内嵌 rt 机器码，全部 rt `.o` 从链接集剔除改为导入引用
    // （dbg 表登记由 rt_library_load 持 OS 句柄完成）；wasm 保持内嵌。
    let is_wasm = mangle::is_wasm_triple(target.unwrap_or(""));
    let shared_rt = if is_wasm {
        None
    } else {
        Some(shared_runtime::build_shared_runtime(
            &rt_base,
            &clang,
            &work_dir,
            target,
            level,
            debug_info,
        )?)
    };
    let shared_rt_input = shared_rt.as_ref().map(|a| a.link_input());

    let runtime_objs =
        prepare_runtime_objects(&rt_base, &clang, &work_dir, level, target, debug_info)?;

    // RFC 016 M4（用户裁决简化 2026-08-03）：相对 `library` 基准 = 执行程序根目录。
    // 动态库链接阶段同样按 `-o` 输出目录解析，与 `compile_to_object` 的 IR 发射
    // 阶段一致（否则 -L 标志在不同基准下查找库文件）。
    let exe_dir = output.parent().unwrap_or_else(|| Path::new("."));
    let native_modules = &resolve_module_library_paths(native_modules, exe_dir);

    let effective_lib_paths = effective_native_lib_paths(native_modules, native_lib_paths, target);

    // RFC 016 M2: 编译期符号验证（与 link_objects_to_executable 一致）。
    // 动态库不强制链接 native 库——未解析符号由运行时加载器解决。
    // 典型场景：QIF 测试动态库由 arc-test-host 加载，host 已链接所有 rt_* 符号。
    // 用户 native 源实现仅进入可执行链接路径（link_objects_to_executable）；
    // 动态库（QIF 测试/插件）阶段无源实现模块 → 空集合。
    if let Err(_detail) = native::verify_symbols::verify_all_native_modules(
        native_modules,
        &effective_lib_paths,
        &std::collections::HashSet::new(),
    ) {
        // 动态库允许 native 符号验证不通过（由 host 运行时提供符号），仅警告。
    }

    let mut link_flags: Vec<String> = mangle::platform_link_flags(target)
        .iter()
        .map(|s| s.to_string())
        .collect();
    // RFC 017 阶段一：插件对共享 runtime 的依赖经 rpath 定位同目录副本
    //（ELF/macOS）；Windows 由加载器默认搜索序（宿主 exe 目录优先）解析，
    // 依赖名相同 → 命中宿主已加载映像（进程单实例）。
    if shared_rt.is_some() {
        for flag in shared_runtime::consumer_rpath_flags(target) {
            link_flags.push(flag);
        }
    }
    for path in &effective_lib_paths {
        link_flags.push(format!("-L{}", path.display()));
    }
    let link_flags_refs: Vec<&str> = link_flags.iter().map(|s| s.as_str()).collect();

    // 动态库排除 WGPU runtime 对象（rt_wgpu_native.o 引用外部 wgpu_* 符号，
    // 这些符号只有 host 进程才提供）。host 不加载 UI/WGPU 相关的动态库。
    let mut link_objs: Vec<&Path> = Vec::with_capacity(objs.len() + runtime_objs.len());
    for o in objs {
        link_objs.push(o);
    }
    for obj in &runtime_objs {
        let name = obj.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "rt_wgpu_native.o"
            || is_platform_runtime_object(name)
            || is_ui_ime_runtime_object(name)
        {
            continue;
        }
        // RFC 017 阶段一：全部 rt `.o` 由共享 runtime 承载，插件仅导入引用
        //（产物不含 rt 机器码——阶段一验收断言）；wasm 保持内嵌。
        if !is_wasm {
            continue;
        }
        link_objs.push(obj.as_path());
    }
    // 共享 runtime 链接输入（导入库 / 共享库本体）置于对象集之后。
    if let Some(input) = &shared_rt_input {
        link_objs.push(input.as_path());
    }

    let export_refs: Vec<&str> = export_symbols.iter().map(|s| s.as_str()).collect();
    let status = optimize::clang_link_shared(
        &clang,
        &link_objs,
        output,
        target,
        &link_flags_refs,
        &export_refs,
    )
    .status()
    .map_err(|e| CodegenError::Llvm(format!("shared link failed: {e}")))?;
    if !status.success() {
        return Err(CodegenError::Llvm("clang shared link failed".into()));
    }

    // RFC 017 阶段一：共享 runtime 落位动态库同目录（缓存 + 硬链接单副本），
    // ELF/macOS 插件 rpath 与 Windows 加载器搜索序均以此为定位点。
    if let Some(artifact) = &shared_rt {
        shared_runtime::stage_shared_runtime(output, artifact);
    }

    // RFC 037 §D7.2: Windows 链接成功后复制 wgpu_native.dll 到输出目录。
    copy_wgpu_native_dll_if_needed(output, target);
    // RFC 026 M1: 同理复制 vendored crypto_native.dll（`rt_crypto_*` ABI 底座）。
    copy_crypto_native_dll_if_needed(output, target);

    Ok(())
}

/// RFC 004 M1: variant 类型 union body 类型选择。
///
/// 从一组 LLVM 类型字符串中选出字节大小最大者作为 union 容器。
/// 启发式排序：ptr > double > i64 > i32 > i16 > i8 > 其他（按字符串长度）。
/// 此启发式足以覆盖 variant payload 的常见类型（int/double/string/ptr/struct）。
fn pick_largest_payload(payload_tys: &[String]) -> String {
    if payload_tys.is_empty() {
        return "[0 x i8]".to_string();
    }
    let rank = |ty: &str| -> u32 {
        match ty {
            "ptr" => 100,
            "i64" => 64,
            "double" => 64,
            "i32" => 32,
            "float" => 32,
            "i16" => 16,
            "i8" => 8,
            "i1" => 1,
            _ => {
                // 复合类型（%struct.* / [N x ...]）按字符串长度作为大小启发式
                50 + ty.len() as u32
            }
        }
    };
    let mut best = payload_tys[0].clone();
    let mut best_rank = rank(&best);
    for ty in payload_tys.iter().skip(1) {
        let r = rank(ty);
        if r > best_rank {
            best = ty.clone();
            best_rank = r;
        }
    }
    best
}

/// Collect source-level symbol info for `.arcdbg` generation (RFC 017 M2).
///
/// For each MIR function, records:
/// - source name (e.g., "Main", "Rectangle::Area")
/// - mangled LLVM symbol name (e.g., "main", "Rectangle_Area")
/// - source file path
/// - source line/col (resolved from `fn_spans` + `line_starts`; 0 = unknown)
fn collect_symbols(
    fns: &[(String, MirCfgBody)],
    file_path: &str,
    fn_spans: &HashMap<String, Span>,
    line_starts: &[u32],
) -> Vec<crate::arcdbg::SymbolInfo> {
    fns.iter()
        .map(|(name, _)| {
            let (line, col) = fn_spans
                .get(name)
                .map(|sp| span_to_line_col(*sp, line_starts))
                .unwrap_or((0, 0));
            crate::arcdbg::SymbolInfo {
                source_name: name.clone(),
                mangled_name: mangle_fn_name(name),
                file: file_path.to_string(),
                line: line as u32,
                col: col as u32,
            }
        })
        .collect()
}

/// Whether class has a vtable（ModuleEmitter `__sinit` 路径与 FnEmitter
/// 函数体路径共用的判定）。
fn class_has_vtable(layouts: &ProgramLayouts, class: &str) -> bool {
    layouts.classes.get(class).is_some_and(|c| c.has_vtable)
}

/// RFC 047（透明对象图迁移 · L3）：vtable 登记表条目计算——**发射端
/// （emit_module）与导出登记端（compile_module_to_dynamic_library）的单点
/// 共享**，两侧条目集必须一致（否则 `.vtable.{T}` 的 /EXPORT 悬空或
/// registry 条目缺席）。
///
/// 范围：本 TU 定义的、含虚方法的 class（external 类的 vtable 定义在别处，
/// 不入本 TU registry）。条目：`{type_name, layout_sig, shape_hash,
/// slot_count}`——vtable 指针不物化（迁移时按名 `.vtable.{T}` 双侧现场
/// 解析）。layout_sig 复用 `entry_layout_signature`（字段布局传递闭包）；
/// shape_hash = 逐虚槽 `name(params):ret` 序列的 FNV-1a-64（捕获方法签名
/// 漂移——字段指纹的盲区，重绑后新代方法将承接旧实例）。
pub(crate) fn vtable_registry_entries(
    layouts: &ProgramLayouts,
    is_external: &dyn Fn(&str) -> bool,
) -> Vec<(String, i64, i64, i32)> {
    let mut entries: Vec<(String, i64, i64, i32)> = Vec::new();
    for (name, cl) in &layouts.classes {
        if !cl.has_vtable || cl.virtual_slots.is_empty() || is_external(name.as_str()) {
            continue;
        }
        let mut shape: u64 = 0xcbf2_9ce4_8422_2325;
        for slot in &cl.virtual_slots {
            let sig = format!(
                "{}({}):{}",
                slot.name,
                slot.params
                    .iter()
                    .map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                slot.ret
            );
            for b in sig.as_bytes() {
                shape ^= *b as u64;
                shape = shape.wrapping_mul(0x100_0000_01b3);
            }
        }
        entries.push((
            name.as_str().to_string(),
            entry_layout_signature(layouts, name.as_str()) as i64,
            shape as i64,
            cl.virtual_slots.len() as i32,
        ));
    }
    entries
}

/// Module-level emitter: orchestrates type declarations, globals, and function emission.
struct ModuleEmitter<'a> {
    layouts: &'a ProgramLayouts,
    is_windows: bool,
    /// RFC 017 阶段一：wasm 目标无共享 runtime（rt_wasm_min 内嵌路径，无
    /// registry ABI），宿主 dbg 表登记注入（`render_host_dbg_registration`）
    /// 据此跳过。
    is_wasm: bool,
    file_path: &'a str,
    /// Byte offset of each line start (RFC 024 M1: span → line/col resolution).
    line_starts: Vec<u32>,
    /// DWARF 5 debug metadata (RFC 031 §2). When disabled, all methods are no-ops.
    dbg: debug_info::DbgMetadata,
    /// Function name → definition span (RFC 031 §2: __arc_dbg_table line/col).
    fn_spans: &'a HashMap<String, Span>,
    /// RFC 004 M1：用户函数返回类型表（key=函数名或 `Owner::Method`，value=cfg.ret）。
    /// 由 `emit_module` 从 fns 构建（与 FnEmitter 的 fn_returns 同源），供
    /// `emit_static_init_expr`（静态字段初始化器直 emit 路径，不经 MIR）解析
    /// 被调方法的真实返回类型——否则嵌套静态方法调用（如
    /// `new SolidColorBrush(Color.Transparent())`）被 emit 成 `call i32`，
    /// 64 位指针截断 → 对象字段垃圾 → 0xC0000005。
    fn_returns: HashMap<String, TypeId>,
    /// Native contract symbol table (RFC 016 M1).
    native_symbols: &'a native::NativeSymbolTable,
    /// RFC 016：运行时加载 native 模块信息（懒解析器符号 + 函数表槽位）。
    /// 由 `build_runtime_infos` 按生效策略分类构建；空表 = 无 runtime 模块。
    runtime_native: &'a native::RuntimeModuleInfos,
    /// RFC 016 M1：native callback 类型表。
    native_callback_table: &'a emit_native_callback::NativeCallbackTable,
    /// RFC 016 M1：模块级 trampoline 累积器。
    /// FnEmitter 在 `try_emit_native_call` 中按需推入，emit_module 末尾统一发射。
    native_trampolines: emit_native_callback::NativeTrampolineAccumulator,
    /// RFC 025 M5：字典枚举幻影类（`DictEnumerator<K,V>`）的 itable +
    /// MoveNext/get_Current 实现发射文本，按 (K,V) 单态化去重。
    /// FnEmitter 在 dict `GetEnumerator` 拦截器中按需推入，emit_module 统一发射
    /// （define 不可嵌套在函数体内）。
    dict_enum_artifacts: std::collections::HashSet<String>,
    /// RFC 016 M3: 契约 struct 类型定义（`%struct.<Name> = type { ... }`）。
    /// 在 emit_module 中紧跟 runtime_decls 之后、native_decls 之前 emit。
    native_struct_types: String,
    /// RFC 023 M1: DI 工厂函数累积器。FnEmitter 通过 `&mut` 引用推入工厂 IR，
    /// emit_module 末尾统一发射到模块级（按 TImpl 去重）。
    di_factories: emit_di::DiFactoryAccumulator,
    /// RFC 032 B2: GenerateTo 属性标记方法元数据表（通用机制）。
    ///
    /// **过渡期（2026-07-19 修订——M4 GenerateTo + Expression 树路径）**：
    /// `arc` crate `qif_collector` 已清理，`generate_to_table.entries` 永远为空。
    /// 本字段保留仅因 `ModuleEmitter::new` 签名引用，`emit_generateto_table`
    /// 不再读取 entries。待新机制（D10.6 + ClassExpression）落地后随
    /// `generate_to_table` 模块一并删除。
    #[allow(dead_code)]
    generate_to_table: &'a GenerateToTable,
    /// RFC 017 M4-link Phase B：跨 `.ao` 包外部符号（来自 typeck
    /// `external_symbols()` API），用于发射 `declare <ret> @<symbol>(...)`。
    /// 来自被链接的 lib.o（external linkage 单一定义来源）。
    external_symbols: &'a [typeck::ExternalSymbolEntry],
    /// RFC 017 M4-link Phase B §D2.1：发射角色——决定全局表符号的 linkage。
    /// - `MainObject`：`@__arc_dbg_table` / `@__generateto_attr_table` 发射为
    ///   external 强符号（默认 `constant [...]`）
    /// - `DynamicLibrary`：同样发射 dbg 表（共享库自含 runtime 就地解析）
    emit_role: crate::EmitRole,
    /// RFC 017 M4: 包元数据——嵌入到动态库 `@__arc_package_meta` 全局符号中，
    /// 供宿主 AssemblyLoadContext.Load() 运行时版本校验。
    /// 仅 DynamicLibrary 角色 + package_meta.name 非空时 emit。
    package_meta: Option<crate::PackageMeta>,
    /// RFC 009 M2：外部类型短名集合——`external_symbols` 中类型条目
    /// （Class/Struct/Interface/Enum/Variant/Module）的**短名**。
    ///
    /// 库 `.o`（`arc publish` 经 `compile_to_object_file_with_core_arc`）只应含
    /// **自身实现**，对 core_arc 外部 Arc 类型以 `declare` 引用；其 typeinfo /
    /// vtable / struct 定义由消费者编译 `std/Arc` 源码提供。若在此发射
    /// `@.typeinfo.{Ext}` / `@.vtable.{Ext}` 会引用外部方法体（mangle_method），
    /// 而这些方法体既不在库 `.o` 定义、又被消费者 tree-shake → 链接 `undefined
    /// symbol`（`UnaryExpression_EvalBool` 等）。故 `emit_struct_types` /
    /// `emit_vtables` / `emit_typeinfos` 跳过本集合。
    ///
    /// 复用既有 ctor/stub 跳过逻辑（`emit_module` 第 6 步）的过滤判据：短名
    /// 与 `layouts.classes` key 一致，直接 `contains` 命中。
    ///
    /// **作用域**：本集合的 typeinfo/vtable/struct 过滤在**任意角色下均生效**——
    /// 主程序（下可执行）与动态库均为**全部实例化类型在自身模块内发射**；
    /// 其中本集合成员（dep 包 / core_arc 源码地基类型）发射为 `linkonce_odr`
    /// + COMDAT（跨 `.o` 去重、供依赖 `external global` 声明解析），其余维持
    ///   `private`（单 TU 内部解析）。`.aopkg` 库角色过滤已随阶段 4 产物收口删撤。
    external_class_names: HashSet<String>,
    /// RFC 038 M2 链接模型（定义包 ↔ 消费方跨包 vtable/typeinfo）：
    /// 本 TU 引用、且**不在本 TU 发射**（属 `external_class_names`，定义包才
    /// 发射）的外部类聚合全局——符号名 → external 声明类型串
    /// （如 `"[5 x ptr]"` / RtTypeInfo 布局串）。
    ///
    /// 任意角色下，活代码构造/判别外部类（依赖包导出面，即
    /// `external_class_names` 成员）时，引用点经统一守卫
    /// （`vtable_global` / `typeinfo_global` / `boxed_struct_vtable_global`）
    /// on-demand 登记于此；`emit_module` 末尾统一发射
    /// `@<sym> = external global <ty>` 声明。external global 在本 TU 内只取
    /// 地址（`store ptr @.vtable.X` 存入对象 vptr 槽 / 作为 `rt_obj_isa`
    /// 判别指针），不 load 内容——类型串仅需指针语义成立；定义由**定义包**
    /// `.o` 的 `linkonce_odr constant` 提供（见 `emit_vtables` /
    /// `emit_typeinfos`），COMDAT 折叠去重、全程序单一定义。
    ///
    /// 登记集合为**链接可解析性安全边界**：仅 `external_class_names` 成员
    /// （dep 导出面 / core_arc 符号表成员，消费方 typeck 必建布局、定义包
    /// `emit_typeinfos`/`emit_vtables` 必发射）可登记；未单态化的模板类
    /// （如注入源码的 `List_T`）不属本集合，其引用仍走「无 vtable → null」
    /// 现状路径（定义包不发射，external 声明必悬空）。
    external_aggregate_refs: std::collections::BTreeMap<String, String>,
    /// RFC 017 阶段一：待回填的基元 typeinfo 槽（GEP 地址文本, prim_id）。
    /// emit_typeinfos 发射含基元槽的 RtFieldInfo/RtPropertyInfo 数组时登记
    /// （数组降级为可写 global、基元槽初值 null）；emit_sinit_and_module_init
    /// 发射 `__arc_module_init` 时统一 `call @rt_typeinfo_prim(id)` 回填真实
    /// 指针——typeinfo 数据符号 static 化后不可跨共享库映像引用。
    pending_prim_fills: Vec<(String, i32)>,
    /// RFC 017 M2：codegen 期动态字符串常量（Entry 符号名 / 异常消息）。
    /// FnEmitter 在发射调用点时 intern；emit_module 末尾统一发射全局。
    string_consts: StringConstAccumulator,
}

impl<'a> ModuleEmitter<'a> {
    fn new(
        layouts: &'a ProgramLayouts,
        is_windows: bool,
        is_wasm: bool,
        file_path: &'a str,
        source: &str,
        debug_info: bool,
        fn_spans: &'a HashMap<String, Span>,
        native_symbols: &'a native::NativeSymbolTable,
        native_callback_table: &'a emit_native_callback::NativeCallbackTable,
        native_struct_types: String,
        generate_to_table: &'a GenerateToTable,
        external_symbols: &'a [typeck::ExternalSymbolEntry],
        emit_role: crate::EmitRole,
        package_meta: Option<crate::PackageMeta>,
        runtime_native: &'a native::RuntimeModuleInfos,
    ) -> Self {
        let line_starts = compute_line_starts(source);
        let dbg = debug_info::DbgMetadata::new(file_path, debug_info);
        // RFC 038 M2：外部类型短名集合（复用 emit_module 第 6 步的过滤判据）。
        // 必须在 new() 中计算——emit_struct_types（emit_module 第 1 步）最早使用。
        let external_class_names: HashSet<String> = external_symbols
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    typeck::ExternalSymbolKind::Class
                        | typeck::ExternalSymbolKind::Struct
                        | typeck::ExternalSymbolKind::Interface
                        | typeck::ExternalSymbolKind::Enum
                        | typeck::ExternalSymbolKind::Variant
                        | typeck::ExternalSymbolKind::Module
                )
            })
            .map(|e| e.name.clone())
            .collect();
        Self {
            layouts,
            is_windows,
            is_wasm,
            file_path,
            line_starts,
            dbg,
            fn_spans,
            fn_returns: HashMap::new(),
            native_symbols,
            native_callback_table,
            native_trampolines: emit_native_callback::NativeTrampolineAccumulator::new(),
            dict_enum_artifacts: std::collections::HashSet::new(),
            native_struct_types,
            di_factories: emit_di::DiFactoryAccumulator::new(),
            generate_to_table,
            external_symbols,
            emit_role,
            package_meta,
            runtime_native,
            external_class_names,
            external_aggregate_refs: std::collections::BTreeMap::new(),
            pending_prim_fills: Vec::new(),
            string_consts: StringConstAccumulator::new(),
        }
    }

    fn emit_module(mut self, fns: &[(String, MirCfgBody)]) -> (String, Vec<StaticInitDiagnostic>) {
        // 静态初始化器直 emit 路径的返回类型表（与 FnEmitter.fn_returns 同源构建）。
        self.fn_returns = fns
            .iter()
            .map(|(n, b)| (n.clone(), b.ret.clone()))
            .collect();
        for (n, b) in fns.iter() {
            if let Some(owner) = &b.owner {
                self.fn_returns
                    .entry(format!("{owner}::{n}"))
                    .or_insert(b.ret.clone());
            }
        }
        let mut out = String::new();
        out.push_str("; Arc LLVM IR module (RFC 020 Phase A)\n");
        out.push_str("; Generated by Arc compiler — do not edit manually.\n\n");

        // 1. Struct type declarations
        out.push_str(&self.emit_struct_types());
        out.push('\n');

        // 1b. Expression tree rodata node type
        out.push_str(&emit_expr_node_type());
        out.push('\n');

        // 2. Runtime declarations
        out.push_str(&emit_runtime_decls(self.is_windows));

        // 2a. Native contract struct types (RFC 016 M3) — 在 declare 之前 emit
        out.push_str(&self.native_struct_types);

        // 2b. Native contract declarations (RFC 016 M1)
        out.push_str(&native::emit_native_decls(
            self.native_symbols,
            self.runtime_native,
        ));

        // 2c. RFC 017 M4-link Phase B：跨 `.ao` 包外部符号声明。
        // 来自 `.ao` `exports[]` 中 Method/StaticMethod/Function 条目，供链接器
        // 从 lib.o 解析符号定义。在 native_decls 之后 emit 保持 declare 段顺序。
        //
        // 本地已定义符号集：注入源码 / 模板单态化在本模块发射的 `define` 符号
        // （`__ctor::Weak_1`、`TaskCompletionSource_bool::SetResult` 等，mangled
        // 后与 declare 同源）。跨包消费时这些符号无需 declare，否则与本地 define
        // 冲突（LLVM `invalid redefinition`）。
        let local_symbols: std::collections::HashSet<String> =
            fns.iter().map(|(name, _)| mangle_fn_name(name)).collect();
        out.push_str(&emit_external_decls(self.external_symbols, &local_symbols));

        // 2b. Source file path global (RFC 017 M1: rt_panic_at 源位置)
        let file_bytes = self.file_path.as_bytes();
        let file_len = file_bytes.len() + 1; // +1 for NUL
        let mut file_escaped = String::new();
        for b in file_bytes {
            if *b == b'\\' {
                file_escaped.push_str("\\\\"); // LLVM IR: \\ 表示一个反斜杠
            } else if b.is_ascii_graphic() || *b == b' ' {
                file_escaped.push(*b as char);
            } else {
                file_escaped.push_str(&format!("\\{:02X}", b));
            }
        }
        out.push_str(&format!(
            "@__arc_file = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
            file_len, file_escaped
        ));

        // RFC 017 M4: 嵌入包元数据——仅动态库且 package_meta.name 非空时 emit。
        // 格式: "name\0version\0edition\0[dep1\0dep2\0...]", 宿主通过 rt_library_get_meta() 读取,
        // AssemblyLoadContext.Load() 做版本兼容校验 + 传递依赖自动加载 (RFC 017 M3 gap ②)。
        if let Some(ref pm) = self.package_meta {
            if !pm.name.is_empty() {
                // 依赖列表追加在 edition 之后，逐项以 NUL 分隔；空依赖时保持旧 3 字段格式。
                let mut meta = format!("{}\0{}\0{}", pm.name, pm.version, pm.edition);
                for dep in &pm.dependencies {
                    meta.push('\0');
                    meta.push_str(dep);
                }
                // 布局指纹子表（RFC 045 D8.1 状态迁移 L1）：依赖段之后的字段，
                // 以 `#layouts:` 自描述前缀标记（std 侧依赖循环「读到空为止」
                // 会吞掉无前缀的后续字段——前缀使其在依赖解析中可识别并转入
                // 指纹解析；子表 `Type1:sig1;Type2:sig2`，':'/';' 分隔——字段
                // 协议为 NUL，子表内不得使用 NUL）。旧产物无此段，运行时按
                // 「未知」保守处理。
                if !pm.layout_sigs.is_empty() {
                    meta.push('\0');
                    meta.push_str("#layouts:");
                    for (i, (ty, sig)) in pm.layout_sigs.iter().enumerate() {
                        if i > 0 {
                            meta.push(';');
                        }
                        meta.push_str(ty);
                        meta.push(':');
                        // i64 十进制（可负）——std 侧 long.TryParse 支持负号，
                        // 但域为 i64：u64 表示会溢出解析失败（实测 14958...e18
                        // > i64::MAX → TryParse 失败 → 表空 → 判定恒拒绝）。
                        meta.push_str(&sig.to_string());
                    }
                }
                // 追加显式空字段终止（双 NUL）：使 rt_library_get_meta_field 能可靠
                // 判定字段越界（否则 strchr 越过末尾 NUL 越界读，返回垃圾指针）。
                meta.push('\0');
                let meta_bytes = meta.as_bytes();
                let meta_len = meta_bytes.len() + 1; // +1 for NUL
                let mut meta_escaped = String::with_capacity(meta_bytes.len());
                for &b in meta_bytes {
                    if b == b'\\' {
                        meta_escaped.push_str("\\\\");
                    } else if b == 0 {
                        meta_escaped.push_str("\\00");
                    } else if b.is_ascii_graphic() || b == b' ' {
                        meta_escaped.push(b as char);
                    } else {
                        meta_escaped.push_str(&format!("\\{:02X}", b));
                    }
                }
                out.push_str(&format!(
                    "; RFC 017 M4: Arc package metadata — name\\00version\\00edition[\\00dep]*\n\
                     @__arc_package_meta = global [{} x i8] c\"{}\\00\"\n",
                    meta_len, meta_escaped,
                ));
            }
        }

        // RFC 047（透明对象图迁移 · L3）：vtable 登记表——条目计算单点在
        // `vtable_registry_entries`（导出登记端 compile_module_to_dynamic_library
        // 共用，两侧一致）。vtable 指针不物化（迁移时按名 `.vtable.{T}` 双侧
        // 现场解析——vtable 全局由本 TU 的虚方法发射必然定义，且经
        // all_exports 显式导出——MSVC 数据符号默认不导出）。
        if let Some(ref pm) = self.package_meta {
            if !pm.name.is_empty() {
                let entries = vtable_registry_entries(
                    self.layouts,
                    &|n: &str| self.external_class_names.contains(n),
                );
                let n = entries.len();
                for (i, (name, _, _, _)) in entries.iter().enumerate() {
                    out.push_str(&format!(
                        "@.str.vtn.{i} = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
                        name.len() + 1,
                        name
                    ));
                }
                let entry_ty = "{ptr, i64, i64, i32}";
                let mut items = String::new();
                for (i, (name, sig, shape, slots)) in entries.iter().enumerate() {
                    if i > 0 {
                        items.push_str(", ");
                    }
                    items.push_str(&format!(
                        "{entry_ty} {{ptr @.str.vtn.{i}, i64 {sig}, i64 {shape}, i32 {slots}}}"
                    ));
                }
                out.push_str(&format!(
                    "; RFC 047: vtable registry (transparent object migration)\n\
                     @__arc_vtable_registry = global [{n} x {entry_ty}] [{items}]\n\
                     @__arc_vtable_registry_count = global i32 {n}\n",
                    n = n,
                ));
            }
        }

        // 3. Vtable globals（基元装箱 vtable 已函数化：`rt_box_vtable(id)` 运行期
        //    查询，RFC 017 阶段一起不再发射 `.vtable.{prim}_Box` 静态常量——
        //    typeinfo 数据符号 static 化后不可跨共享库映像引用）。
        out.push_str(&self.emit_vtables());
        out.push_str(&self.emit_boxed_struct_vtables());
        let fn_names: HashSet<String> = fns.iter().map(|(n, _)| mangle_fn_name(n)).collect();
        out.push_str(&self.emit_itables(&fn_names));
        out.push('\n');

        // 3b. RFC 006 M4：静态字段全局变量（在 vtable 之后、函数定义之前 emit，
        //     以便 `__sinit` 与 main 中的 store/load 能前向引用）。
        out.push_str(&self.emit_static_field_globals());

        // 3c. RFC 017 §2.3：模块根元数据表（`--dynamic` 共享库 codegen 自动发射；
        //     静态字段 class 引用槽位地址表，运行时 load 后自动登记为模块根）。
        out.push_str(&self.emit_module_roots_table());

        // 4. Pre-collect string literals so all FnEmitters share the same global names
        let mut string_literals: Vec<String> = Vec::new();
        let mut string_seen: HashMap<String, String> = HashMap::new();
        for (_, body) in fns {
            collect_string_literals(body, &mut string_literals, &mut string_seen);
        }

        // 4c. Collect expression trees for rodata emission
        let mut expr_trees: Vec<(String, ast::ExpressionTree)> = Vec::new();
        for (_, body) in fns {
            expr_trees.extend(collect_expr_trees(body));
        }
        // Intern tree-embedded string constants into the shared string pool
        // before rodata emission. This replaces the previous hash-based naming
        // scheme that was vulnerable to collisions (C2 in the foundation audit).
        for (_, tree) in &expr_trees {
            expr_rodata::intern_tree_strings(tree, &mut string_literals, &mut string_seen);
        }

        // 4b. Collect async function names so call sites know to use `ptr` return type
        let async_fns: HashSet<String> = fns
            .iter()
            .filter(|(_, b)| b.is_async)
            .map(|(n, _)| n.clone())
            .collect();

        // 4c. RFC 004 M1: 预收集所有用户函数返回类型，供 `emit_call_typed`
        //     在 user function call 路径查询真实返回类型。`emit_rvalue`
        //     （不带 typed 入口）传默认 `Int` 作 `expected`，对返回 bool/string/long
        //     等的函数（如 `bool Same<T>(T,T)`）会生成 `call i32 @Same_int(...)`
        //     与实际 `define i1 @Same_int(...)` 类型错配，导致 LLVM verifier 报错。
        //     通过此表回填真实返回类型，避免依赖 `expected` 的兜底语义。
        let mut fn_returns: HashMap<String, TypeId> = fns
            .iter()
            .map(|(n, b)| (n.clone(), b.ret.clone()))
            .collect();
        // RFC 009 M5 fix: class methods in fn_returns are keyed by bare
        // method name ("DoWork"), but emit_method_call_typed looks up by
        // "Class::Method" format.  Add dual entries so both lookup paths work.
        for (n, b) in fns.iter() {
            if let Some(owner) = &b.owner {
                let full = format!("{owner}::{n}");
                fn_returns.entry(full).or_insert(b.ret.clone());
            }
        }

        // 4d. RFC 017 M4-link Phase B：发射 `$<name> = comdat any` 模块级声明。
        //
        // **Windows COFF 必需**：`linkonce_odr` linkage 在 COFF 目标上不会自动
        // 创建 COMDAT 段，lld-link 跨 `.o` 链接时仍报 `duplicate symbol`。必须
        // 在模块级显式声明 `$<name> = comdat any`，并在 `define` 行附加 `comdat`
        // 属性，链接器才能按 COMDAT group 跨 `.o` 去重。Linux ELF 上 `comdat`
        // 指令被映射为 section group，与隐式 `linkonce_odr` 行为等价（无副作用）。
        //
        // 收集范围：所有 `MirCfgBody.linkage == LinkonceOdr` 的函数（去重 mangle 名）
        // + 第 6 步将为非 external_symbols 类发射的默认 `__ctor::<Class>`（无显式
        // ctor 的类）。后者需在此预先收集，与第 6 步保持名称一致。
        let comdat_names = self.collect_comdat_names(fns);
        out.push_str(&emit_comdat_decls(&comdat_names));

        // 4e. RFC 015 Phase B.7：模块内 call-graph `nounwind` 推断。
        //     已知 nounwind 被调方 = 模块内 nounwind ∪ `rt_*` 白名单（除
        //     RT_MAY_THROW）∪ libc/llvm leaf；虚/间接/未知外部仍 may-throw。
        let nounwind_map = analyze_module_nounwind(fns);

        // 5. Function definitions
        out.push_str("; ---- Function definitions ----\n");
        // Dedup by mangled name: upstream monomorphization (typeck/MIR) may
        // produce duplicate TypedFn entries when the same generic instantiation
        // is reached via multiple paths (e.g. `Enumerable_Any` instantiated
        // both via extension-method resolution and via static-class emission).
        // LLVM IR forbids duplicate `define` of the same symbol, so skip later
        // occurrences. This is a defensive fix at the codegen boundary; the
        // upstream duplicate-instantiation root cause is tracked separately.
        let mut emitted_fns: HashSet<String> = HashSet::new();
        for (name, body) in fns {
            if !emitted_fns.insert(mangle_fn_name(name)) {
                continue;
            }
            // RFC 031 §2: create DISubprogram metadata for this function.
            let subprogram_id = self.create_subprogram(name, body);
            let mut fn_emitter = FnEmitter::new(
                body,
                self.layouts,
                &self.external_class_names,
                &mut self.external_aggregate_refs,
                &string_seen,
                &mut self.string_consts,
                &async_fns,
                self.is_windows,
                &self.line_starts,
                &mut self.dbg,
                subprogram_id,
                self.native_symbols,
                self.runtime_native,
                self.native_callback_table,
                &mut self.native_trampolines,
                &mut self.dict_enum_artifacts,
                &mut self.di_factories,
                &fn_returns,
                &nounwind_map,
            );
            out.push_str(&fn_emitter.emit_function(name));
            out.push('\n');
        }

        // 5b. RFC 023 M1: DI 工厂函数。FnEmitter 在 emit_function 期间累积工厂 IR
        //     （由 Add 拦截器通过 ensure_factory_generated 推入）。在所有用户函数
        //     发射完成后统一输出到模块级，按 TImpl 去重（DiFactoryAccumulator.names）。
        //     工厂闭包全局常量（注册路径零 malloc）与 immortal RuntimeType 全局
        //     （依赖解析零分配）先行发射——LLVM 全局无先声明次序要求，前向引用安全。
        if !self.di_factories.closure_irs.is_empty() {
            out.push_str("; ---- RFC 023 冲刺批次一: DI factory closure globals ----\n");
            for closure_ir in &self.di_factories.closure_irs {
                out.push_str(closure_ir);
            }
        }
        if !self.di_factories.runtime_type_irs.is_empty() {
            out.push_str("; ---- RFC 023 M1: immortal RuntimeType globals ----\n");
            for rt_ir in &self.di_factories.runtime_type_irs {
                out.push_str(rt_ir);
            }
        }
        if !self.di_factories.irs.is_empty() {
            out.push_str("; ---- RFC 023 M1: DI factory functions ----\n");
            for factory_ir in &self.di_factories.irs {
                out.push_str(factory_ir);
            }
        }

        // 5b. RFC 016 M1: native callback trampoline 函数。FnEmitter 在
        //     try_emit_native_call 期间按需累积 trampoline IR，将 Arc 函数
        //     指针适配到 C ABI callback 类型签名。在所有用户函数后统一发射，
        //     避免在函数体内嵌套 define（LLVM IR 不允许嵌套函数定义）。
        if !self.native_trampolines.is_empty() {
            out.push_str("; ---- RFC 016 M1: native callback trampolines ----\n");
            // RFC 016 M2：有捕获 lambda TLS 回调 slot 数上限校验（编译期拒绝超限）。
            if self.native_trampolines.slot_count() > 16 {
                panic!(
                    "codegen: too many captured native callbacks ({}) — exceeds RT_FFI_MAX_CALLBACK_SLOTS (16)",
                    self.native_trampolines.slot_count()
                );
            }
            for tramp_ir in self.native_trampolines.irs() {
                out.push_str(tramp_ir);
            }
        }

        // 5b2. RFC 016: runtime-loaded native modules（懒解析器 globals + 函数）。
        //      在所有用户函数/trampoline 后统一发射（define 不可嵌套）。
        out.push_str(&native::emit_runtime_load_support(self.runtime_native));

        // 5b3. RFC 025 M5：字典枚举幻影类 itable + MoveNext/get_Current 实现。
        //     FnEmitter 在 dict `GetEnumerator` 拦截器中按 (K,V) 去重推入；
        //     itable 槽序与 `iface_method_index`（IEnumerator：MoveNext 方法槽 0、
        //     Current 属性 getter 槽 1）一致。
        if !self.dict_enum_artifacts.is_empty() {
            out.push_str("; ---- RFC 025 M5: DictEnumerator phantom itables ----\n");
            let mut artifacts: Vec<&String> = self.dict_enum_artifacts.iter().collect();
            artifacts.sort();
            for artifact in artifacts {
                out.push_str(artifact);
                out.push('\n');
            }
        }

        // 5c. RFC 004 M2: Dictionary 用户类型键 trampoline。
        //     扫描所有 `__ctor::Dictionary_*` 函数名，提取用户类型 K 后缀
        //     （非基元、非 string），按 K 去重发射 `@__dict_hash_{K}` 与
        //     `@__dict_eq_{K}` trampoline。trampoline 调用用户类型的
        //     `K_GetHashCode` / `K_Equals` 静态方法，实现零装箱哈希。
        //     若用户类型未实现这些方法，链接器会报 undefined symbol
        //     （typeck M2 应在编译期拦截，此处为防御性兜底）。
        let mut user_key_types: HashSet<String> = HashSet::new();
        for (name, _) in fns {
            let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
            let class_name = class_name.split("::").next().unwrap_or(class_name);
            if let Some((k_suf, _)) = parse_dict_kv(class_name) {
                if dict_kv_is_user_type(&k_suf, self.layouts) {
                    user_key_types.insert(k_suf);
                }
            }
        }
        if !user_key_types.is_empty() {
            out.push_str("; ---- RFC 004 M2: Dictionary user-type key trampolines ----\n");
            for k in &user_key_types {
                out.push_str(&self.emit_dict_user_trampolines(k));
            }
            out.push('\n');
        }

        // 5d. RFC 017 M2: 动态库 Entry wrapper 函数。
        // 对 `kind="library" + dynamic=true` 的编译单元，扫描顶层 `Entry` 方法，
        // 为每个生成 `__arc_entry_{TP}_{TR}` C ABI 导出符号（零装箱 monomorphized wrapper）。
        // 仅在 DynamicLibrary 角色下发射（MainObject 不需要入口 wrapper）。
        if matches!(self.emit_role, crate::EmitRole::DynamicLibrary) {
            let wrappers = self.emit_entry_wrappers(fns);
            if !wrappers.is_empty() {
                out.push_str("; ---- RFC 017 M2: Entry wrappers (dynamic library) ----\n");
                out.push_str(&wrappers);
                out.push('\n');
            }
        }

        // 6. Default constructors for classes without explicit ctors.
        //    `emit_new` always calls `__ctor::Class`; emit an empty body when none was defined.
        //
        //    RFC 017 M4-link Phase B §D2.1：按所有权规则过滤发射。
        //    - **external_symbols 中的类跳过**：消费方仅 `declare` 即可，定义来自 lib.o
        //      （遵循 §D2 已决表「`.ao` exports 外部符号 → declare」原则）。消费方若
        //      补 `define` 会与 lib.o 的 `define` 冲突——`duplicate symbol: __ctor_X`
        //    - **其他所有类（含 std 库依赖 + 用户源码 + builtin facade）均用 linkonce_odr**：
        //      std 库依赖类（如 Object/TypeId）在 main.o 与 lib.o 中都被 emit_module 发射，
        //      linkonce_odr 弱符号允许跨 .o 重复，链接器选一个；
        //      用户源码类与 builtin facade 类的 __ctor 只在自身 .o 中被发射，
        //      linkonce_odr 在单一来源场景等价于 external（链接器选那一个）。
        //      统一用 linkonce_odr 简化决策矩阵，避免「std 库类 external → 重复」陷阱。
        let defined_ctors: HashSet<String> = fns
            .iter()
            .filter(|(n, _)| n.contains("__ctor"))
            .map(|(n, _)| n.clone())
            .collect();
        // 构造 external_symbols 中的类名集合（Class/Struct/Interface/Enum/Variant/Module）
        // ——这些类的 __ctor 定义由 lib.o 提供，消费方不发射。
        let external_class_names: HashSet<&str> = self
            .external_symbols
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    typeck::ExternalSymbolKind::Class
                        | typeck::ExternalSymbolKind::Struct
                        | typeck::ExternalSymbolKind::Interface
                        | typeck::ExternalSymbolKind::Enum
                        | typeck::ExternalSymbolKind::Variant
                        | typeck::ExternalSymbolKind::Module
                )
            })
            .map(|e| e.name.as_str())
            .collect();
        for class_name in self.layouts.classes.keys() {
            // 跳过 external_symbols 中的类——其 __ctor 定义来自 lib.o
            if external_class_names.contains(class_name.as_str()) {
                continue;
            }
            let ctor_name = format!("__ctor::{class_name}");
            // 跳过 stub 处理的类（stub 自带 linkonce_odr + comdat，无需默认空 ctor）
            if emit_stubs::class_is_stub_handled(&ctor_name) {
                continue;
            }
            if !defined_ctors.contains(&ctor_name) {
                let mangled = mangle_fn_name(&ctor_name);
                let subprogram_id = self.create_subprogram(&ctor_name, &fns[0].1);
                // Only emit !dbg when debug info is enabled (subprogram_id > 0).
                // When disabled, there are no metadata nodes — !dbg !0 would be undefined.
                let dbg_attr = if subprogram_id > 0 {
                    format!(" !dbg !{subprogram_id}")
                } else {
                    String::new()
                };
                // 所有非 external_symbols 的类统一用 linkonce_odr 弱符号发射默认 ctor
                // （std 库依赖跨 .o 重复可消解；用户源码与 builtin facade 单一来源场景
                // linkonce_odr 等价 external）。
                //
                // RFC 017 M4-link Phase B：附加 `comdat` 属性——Windows COFF 上
                // `linkonce_odr` 若无 `comdat` 指令，lld-link 仍报 `duplicate symbol`。
                // 模块级 `$<mangled> = comdat any` 声明由 `emit_comdat_decls` 第 4d 步
                // 统一收集发射（包括本步骤生成的默认 ctor）。
                out.push_str(&format!(
                    "define linkonce_odr void @{mangled}(ptr %self) comdat{dbg_attr} {{\nentry:\n  ret void\n}}\n\n"
                ));
            }
        }

        // 6b. Default constructors for structs without explicit ctors.
        //     struct 是值类型，在栈上分配（alloca，非 calloc），默认 ctor 与 class 一致：
        //     空函数体。字段零初始化由调用方通过 `store zeroinitializer` 或显式 ctor 保证。
        for struct_name in self.layouts.structs.keys() {
            if external_class_names.contains(struct_name.as_str()) {
                continue;
            }
            let ctor_name = format!("__ctor::{struct_name}");
            if !defined_ctors.contains(&ctor_name) {
                let mangled = mangle_fn_name(&ctor_name);
                out.push_str(&format!(
                    "define linkonce_odr void @{mangled}(ptr %self) comdat {{\nentry:\n  ret void\n}}\n\n"
                ));
            }
        }

        // 6a. RFC 018 M2 step 6: abstract method stub 发射。
        //
        // abstract property（如 `Type.TypeId`）被 typeck 注册为 `get_X` abstract 方法，
        // vtable 引用 `@{Class}_get_X` 符号。但 abstract 方法无方法体，不在 MIR fns 中，
        // codegen 不发射函数定义 → undefined symbol。
        //
        // 防御性兜底：为 vtable 中未被 MIR fns 定义的 slot 发射返回默认值的 stub。
        // 正常情况下派生类应 override（如 RuntimeType override Type.TypeId），
        // vtable 分派会命中派生类实现，stub 永不执行。stub 仅用于：
        //   1. abstract 方法未被派生类 override（编程错误，应 typeck 拦截）
        //   2. 链接期符号解析（避免 undefined symbol）
        //
        // **vtable slot 重定向（step 6 核心）**：通过 Type/TypeInfo 基类引用调用
        // getter 时，codegen 拦截器（emit_method_call_typed）直接走拦截路径，
        // 绕过 vtable 分派，故 stub 不会被调用。
        let defined_fns: HashSet<String> = fns.iter().map(|(n, _)| mangle_fn_name(n)).collect();
        let mut emitted_stubs: HashSet<String> = HashSet::new();
        for class in self.layouts.classes.values() {
            if !class.has_vtable {
                continue;
            }
            for slot in &class.virtual_slots {
                let impl_class = slot.impl_class.clone();
                let fn_name = mangle_fn_name(&slot.link_name);
                // 跳过：MIR fns 已定义 / 本次已发射 stub / external_symbols 中的类
                if defined_fns.contains(&fn_name)
                    || !emitted_stubs.insert(fn_name.clone())
                    || self
                        .external_symbols
                        .iter()
                        .any(|e| e.name == impl_class.as_str())
                {
                    continue;
                }
                let llvm_ret = simple_llvm_ret_ty(&slot.ret);
                let ret_stmt = if llvm_ret == "void" {
                    "ret void".to_string()
                } else {
                    format!("ret {llvm_ret} zeroinitializer")
                };
                out.push_str(&format!(
                    "${fn_name} = comdat any\n\
                     define linkonce_odr {llvm_ret} @{fn_name}(ptr %self) comdat {{\nentry:\n  {ret_stmt}\n}}\n\n"
                ));
            }
        }

        // 6b. Expression tree rodata globals
        out.push_str(&emit_expr_tree_globals(&expr_trees, &string_seen));

        // 7. String literal globals (appended at end — LLVM allows forward references)
        out.push_str(&emit_string_globals(&string_literals));

        // 7a. RFC 017 M2: Assembly.Entry 符号名 / 异常消息字符串常量。
        //     FnEmitter 在发射 Entry 调用点时 intern；此处统一发射全局。
        if !self.string_consts.is_empty() {
            out.push_str(&self.string_consts.render());
        }

        // 7c. RFC 006 M4：__sinit_<Class> 静态初始化器函数 + @__arc_module_init 聚合调用。
        //     必须在 string globals 之后 emit，以便字符串字面量初始化器可引用 string pool。
        //     main 入口生成器（emit_fn.rs）在 entry 块开头插入 `call void @__arc_module_init()`。
        //     拓扑序基于 `fns`（MIR 函数体）穿透被调函数收集静态字段依赖（static_init_deps）。
        let (sinit_ir, mut sinit_diags) = self.emit_sinit_and_module_init(fns);
        out.push_str(&sinit_ir);

        // 7c2. RFC 006 A3 S3：`__lazy_init_<Class>` 惰性初始化器函数。
        //     必须在 string globals 之后 emit（惰性初始化器可能引用 string pool），
        //     供 StaticField 读取 guard 的慢路径调用。
        //     与急切路径共用 emit_static_init_expr——其表达式形态诊断（arc-sinit-003）
        //     合并入同一诊断通道返回。
        let (lazy_ir, mut lazy_diags) = self.emit_lazy_init_functions();
        out.push_str(&lazy_ir);
        sinit_diags.append(&mut lazy_diags);

        // 7b. RFC 017 M2: Embedded debug symbol table for runtime symbolization.
        //     Always emit the table (even without -g, with count=0) so the
        //     runtime's extern references always resolve at link time.
        //     When -g is enabled, the table contains (fn_ptr, name, file, line)
        //     entries; when disabled, count=0 and lookups return no info.
        out.push_str(&self.emit_debug_table(fns));

        // 8. DWARF 5 debug metadata (RFC 017 M2)
        self.dbg.finalize();
        out.push_str(&self.dbg.render());

        // Zero-cost EH (RFC 010 milestone ⑦): known-nounwind `declare`d
        // externals must carry `nounwind` so a plain `call` inside a cleanup
        // funclet (finally body during unwind) is valid Windows EH IR.
        // Without this, clang drops the funclet body for non-nounwind calls.
        // NOTE: temporarily disabled to isolate async_lambda class-capture regression.
        // out = mark_nounwind_decls(out);

        // RFC 038 M2：发射本 TU 登记的外部类聚合全局声明（`@.typeinfo.{Ext}` /
        // `@.vtable.{Ext}` / `@.vtable.{T}_Box`）。登记贯穿第 3 步
        // （`emit_typeinfos`）与函数体 / `__sinit` 发射阶段，须待全部完成后
        // 在模块末尾统一输出（LLVM 允许前向引用）。
        out.push_str(&self.emit_external_aggregate_decls());

        (out, sinit_diags)
    }

    /// RFC 017 M4-link Phase B：收集模块内所有需要 `comdat` 声明的符号名。
    ///
    /// 包括两类：
    /// 1. **`MirCfgBody.linkage == LinkonceOdr` 的函数**：从 `fns` 中过滤，
    ///    取 mangle 名并去重（与第 5 步的 `emitted_fns` 去重逻辑一致）。
    /// 2. **第 6 步将发射的默认 `__ctor::<Class>`**：扫描 `layouts.classes`，
    ///    排除 `external_symbols` 中已含的类（其 ctor 定义来自 lib.o）和
    ///    `fns` 中已有显式 `__ctor::` 的类（走 FnEmitter 路径，已在第 1 类
    ///    收集）。剩下的是第 6 步会用 `linkonce_odr` 发射默认空 ctor 的类。
    ///
    /// 返回的 `Vec<String>` 已按名排序去重，便于稳定的 IR 输出（测试对比）。
    fn collect_comdat_names(&self, fns: &[(String, MirCfgBody)]) -> Vec<String> {
        let mut names: HashSet<String> = HashSet::new();

        // 第 1 类：LinkonceOdr 函数（含 builtin facade 方法 / 单态化实例 /
        // 显式 __ctor:: 等）。去重 mangle 名，与第 5 步的 emitted_fns 一致。
        //
        // **注意**：try_emit_stub 处理的函数（builtin collection 方法及
        // 其内嵌子 stub，如 ListEnumerator<T>::MoveNext）由 stub_linkonce
        // 内联发射 `$<name> = comdat any` 声明，不应重复收集到模块级
        // comdat 段（会导致 COMDAT 重定义）。
        for (name, body) in fns {
            if body.linkage == mir::Linkage::LinkonceOdr {
                // 跳过 stub 处理的函数：其 comdat 由 stub_linkonce 内联发射
                if !emit_stubs::class_is_stub_handled(name) {
                    names.insert(mangle_fn_name(name));
                }
            }
        }

        // 第 2 类：第 6 步将为非 external_symbols 类发射的默认 __ctor::Class。
        // 与第 6 步过滤条件保持一致：跳过 external_symbols 类、跳过 stub 处理类
        //（其 ctor 由 emit_stubs + stub_linkonce 内联发射 comdat）、跳过已有显式
        // __ctor:: 的类。剩余类的 `__ctor::<Class>` 会被第 6 步用 `linkonce_odr`
        // 发射空体，需在此预先声明 comdat。
        let defined_ctors: HashSet<String> = fns
            .iter()
            .filter(|(n, _)| n.contains("__ctor"))
            .map(|(n, _)| n.clone())
            .collect();
        let external_class_names: HashSet<&str> = self
            .external_symbols
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    typeck::ExternalSymbolKind::Class
                        | typeck::ExternalSymbolKind::Struct
                        | typeck::ExternalSymbolKind::Interface
                        | typeck::ExternalSymbolKind::Enum
                        | typeck::ExternalSymbolKind::Variant
                        | typeck::ExternalSymbolKind::Module
                )
            })
            .map(|e| e.name.as_str())
            .collect();
        for class_name in self.layouts.classes.keys() {
            if external_class_names.contains(class_name.as_str()) {
                continue;
            }
            let ctor_name = format!("__ctor::{class_name}");
            // 跳过 stub 处理的类：其 ctor 由 emit_stubs 路径内联发射 comdat
            // 声明（stub_linkonce），与第 6 步一致避免 comdat 重定义。
            if emit_stubs::class_is_stub_handled(&ctor_name) {
                continue;
            }
            if !defined_ctors.contains(&ctor_name) {
                names.insert(mangle_fn_name(&ctor_name));
            }
        }
        // 第 2b 类：第 6b 步将为非 external_symbols struct 发射的默认 ctor。
        for struct_name in self.layouts.structs.keys() {
            if external_class_names.contains(struct_name.as_str()) {
                continue;
            }
            let ctor_name = format!("__ctor::{struct_name}");
            if !defined_ctors.contains(&ctor_name) {
                names.insert(mangle_fn_name(&ctor_name));
            }
        }

        // 第 3 类数据表：MainObject / DynamicLibrary 统一收集 `__arc_dbg_*`——
        // 两角色均发射 dbg 表（见 emit_debug_table），可声明链接入 comdat。

        let mut sorted: Vec<String> = names.into_iter().collect();
        sorted.sort();
        sorted
    }

    /// RFC 017 M4-link Phase B §D2.1：MainObject 全局 dbg 表 linkage 前缀。
    fn table_linkage_prefix(&self) -> &'static str {
        ""
    }

    /// Emit the embedded debug symbol table (RFC 017 M2 / L2 StackTrace).
    ///
    /// Generates a global `__arc_dbg_table` array of `{ptr, ptr, ptr, i32, i32}`
    /// entries mapping function pointers to (name, file, line, col). The runtime
    /// reads this table directly — no file I/O needed, works on all platforms
    /// (Windows MSVC/MinGW, POSIX).
    ///
    /// **Always populated**（MainObject）for `Exception.StackTrace` symbolization,
    /// independent of DWARF `-g`.
    ///
    /// RFC 017 M4-link Phase B §D2.1（包边界 MVP）：
    /// - **MainObject / DynamicLibrary**：external 强符号（`rt_debug.c` 的
    ///   `extern` 解析点）。动态库（`arc build --dynamic`）自含 runtime，内嵌
    ///   `rt_debug.o` 硬引用本表，Windows PE 链接须就地解析。
    ///
    /// `.aopkg` 发布路径（`.ao` 库产物）已随阶段 4 产物收口删撤，不再有
    /// 「库角色跳过 dbg 表」分支——两角色统一发射，避免 COFF `duplicate
    /// symbol` 的库/main 并存场景也随之消除。
    fn emit_debug_table(&self, fns: &[(String, MirCfgBody)]) -> String {
        let mut out = String::new();
        out.push_str("; ---- RFC 017 M2: Embedded debug symbol table ----\n");

        // Struct type: { fn_ptr, name_ptr, file_ptr, line, col }
        out.push_str("%struct.ArcDbgEntry = type { ptr, ptr, ptr, i32, i32 }\n");

        let linkage = self.table_linkage_prefix();
        if fns.is_empty() {
            out.push_str(&format!(
                "@__arc_dbg_table = {linkage}constant [0 x %struct.ArcDbgEntry] []\n"
            ));
            out.push_str(&format!("@__arc_dbg_count = {linkage}constant i32 0\n\n"));
            return out;
        }

        // Emit name string constants for each function.
        let mut name_globals: Vec<String> = Vec::new();
        for (i, (name, _)) in fns.iter().enumerate() {
            let global_name = format!("@.arcdbg.name.{i}");
            let escaped = string_pool::escape_llvm_string(name.as_bytes());
            let len = name.len() + 1; // +1 for NUL
            out.push_str(&format!(
                "{global_name} = private unnamed_addr constant [{len} x i8] c\"{escaped}\\00\"\n"
            ));
            name_globals.push(global_name);
        }

        // Emit file path string constant (shared by all functions in this unit).
        let file_escaped = string_pool::escape_llvm_string(self.file_path.as_bytes());
        let file_len = self.file_path.len() + 1;
        out.push_str(&format!(
            "@.arcdbg.file = private unnamed_addr constant [{file_len} x i8] c\"{file_escaped}\\00\"\n"
        ));

        let count = fns.len();
        out.push_str(&format!(
            "@__arc_dbg_table = {linkage}constant [{count} x %struct.ArcDbgEntry] [\n"
        ));
        for (i, (name, _)) in fns.iter().enumerate() {
            let mangled = mangle_fn_name(name);
            let (line, col) = self
                .fn_spans
                .get(name)
                .map(|sp| span_to_line_col(*sp, &self.line_starts))
                .unwrap_or((0, 0));
            let comma = if i + 1 < count { "," } else { "" };
            out.push_str(&format!(
                "  %struct.ArcDbgEntry {{ ptr @{mangled}, ptr {name_global}, ptr @.arcdbg.file, i32 {line}, i32 {col} }}{comma}\n",
                name_global = name_globals[i]
            ));
        }
        out.push_str("]\n");

        out.push_str(&format!(
            "@__arc_dbg_count = {linkage}constant i32 {count}\n\n"
        ));

        out
    }

    /// RFC 017 M2: 动态库 Entry wrapper 生成。
    ///
    /// 扫描 `fns` 中名为 `Entry` 的顶层函数，为签名匹配 `TResult?(TParameter?)`
    /// 的每个 Entry 生成 C ABI 导出符号 `__arc_entry_{TP}_{TR}_{TP_sig}_{TR_sig}`
    /// （0 参为 `__arc_entry__{TR_id}_{TR_sig}`；指纹段见
    /// [`entry_layout_signature`]）。
    ///
    /// **任意类型支持**：
    /// - **struct 类型**：null → zeroinit / 非 null → memcpy → 调用 → 堆分配返回
    /// - **class 类型**：ArcHeader* 透传（零包装），调用方通过 ARC inc/dec 管理
    ///
    /// wrapper 无需感知具体类型布局——依赖 LLVM struct 类型定义 + mangle 函数签名。
    fn emit_entry_wrappers(&self, fns: &[(String, MirCfgBody)]) -> String {
        let mut out = String::new();

        for (name, body) in fns {
            let is_entry = name == "Entry" || name.ends_with("::Entry");
            // RFC 017 M3: 支持 0-param Entry() 和 1-param Entry(T?), 拒绝多参数
            if !is_entry || body.params.len() > 1 {
                continue;
            }

            let extract_struct_name = |ty: &TypeId| -> Option<String> {
                match ty {
                    TypeId::Named(s) => Some(s.to_string()),
                    TypeId::Nullable { ref inner } => match inner.as_ref() {
                        TypeId::Named(s) => Some(s.to_string()),
                        _ => None,
                    },
                    _ => None,
                }
            };
            let Some(tr_name) = extract_struct_name(&body.ret) else {
                continue;
            };
            let tr_id = emit_rvalue::type_name_to_id(&tr_name);
            // 布局指纹段：同名类型布局漂移 → 符号不匹配 → 宿主加载期显式
            // EntryPointNotFound（替代 ABI 静默错配）。
            let tr_sig = entry_layout_signature(self.layouts, &tr_name);
            let tr_is_class = self.layouts.classes.contains_key(tr_name.as_str());
            let entry_mangled = mangle_fn_name(name);

            if body.params.is_empty() {
                // === 0-param Entry(): → TResult? ===
                let wrapper_name = format!("__arc_entry__{tr_id}_{tr_sig}");
                if tr_is_class {
                    out.push_str(&format!(
                        "; RFC 017 M3: Entry wrapper (0-param, class) {name} → {tr_name}\n\
                         define ptr @{wrapper_name}(ptr %unused) {{\n\
                         entry:\n\
                         \x20 %ret_val = call ptr @{entry_mangled}()\n\
                         \x20 ret ptr %ret_val\n\
                         }}\n\n",
                    ));
                } else {
                    let tr_llvm = format!("%struct.{tr_name}");
                    out.push_str(&format!(
                        "; RFC 017 M3: Entry wrapper (0-param, struct) {name} → {tr_name}\n\
                         define ptr @{wrapper_name}(ptr %unused) {{\n\
                         entry:\n\
                         \x20 %ret_val = call {tr_llvm} @{entry_mangled}()\n\
                         \x20 %ret_slot = alloca {tr_llvm}\n\
                         \x20 store {tr_llvm} %ret_val, ptr %ret_slot\n\
                         \x20 %tr_sz = ptrtoint (ptr getelementptr inbounds ({tr_llvm}, ptr null, i32 1)) to i64\n\
                         \x20 %heap_ret = call ptr @malloc(i64 %tr_sz)\n\
                         \x20 call void @llvm.memcpy.p0.p0.i64(ptr %heap_ret, ptr %ret_slot, i64 %tr_sz, i1 false)\n\
                         \x20 ret ptr %heap_ret\n\
                         }}\n\n",
                    ));
                }
                continue;
            }

            // === 1-param Entry(T?) → TResult? ===
            let param_ty = &body.params[0].1;
            let Some(tp_name) = extract_struct_name(param_ty) else {
                continue;
            };
            let tp_id = emit_rvalue::type_name_to_id(&tp_name);
            let tp_sig = entry_layout_signature(self.layouts, &tp_name);
            let wrapper_name = format!("__arc_entry_{tp_id}_{tr_id}_{tp_sig}_{tr_sig}");
            let tp_is_class = self.layouts.classes.contains_key(tp_name.as_str());

            if tp_is_class && tr_is_class {
                // === class → class：纯 ptr 透传 ===
                out.push_str(&format!(
                    "; RFC 017 M2: Entry wrapper (class) {name} — {tp_name} → {tr_name}\n\
                     define ptr @{wrapper_name}(ptr %arg_ptr) {{\n\
                     entry:\n\
                     \x20 %ret_val = call ptr @{entry_mangled}(ptr %arg_ptr)\n\
                     \x20 ret ptr %ret_val\n\
                     }}\n\n",
                ));
            } else {
                // === struct 类型：memcpy marshal ===
                let tp_llvm = format!("%struct.{tp_name}");
                let tr_llvm = format!("%struct.{tr_name}");

                out.push_str(&format!(
                    "; RFC 017 M2: Entry wrapper (struct) {name} — {tp_name} → {tr_name}\n\
                     define ptr @{wrapper_name}(ptr %args_ptr) {{\n\
                     entry:\n\
                     \x20 %arg_slot = alloca {tp_llvm}\n\
                     \x20 %tp_sz_ptr = getelementptr inbounds {tp_llvm}, ptr null, i32 1\n\
                     \x20 %tp_sz = ptrtoint ptr %tp_sz_ptr to i64\n\
                     \x20 %tr_sz_ptr = getelementptr inbounds {tr_llvm}, ptr null, i32 1\n\
                     \x20 %tr_sz = ptrtoint ptr %tr_sz_ptr to i64\n\
                     \x20 %is_null = icmp eq ptr %args_ptr, null\n\
                     \x20 br i1 %is_null, label %zero_init, label %copy_in\n\
                     \n\
                     zero_init:\n\
                     \x20 call void @llvm.memset.p0.i64(ptr %arg_slot, i8 0, i64 %tp_sz, i1 false)\n\
                     \x20 br label %do_call\n\
                     \n\
                     copy_in:\n\
                     \x20 call void @llvm.memcpy.p0.p0.i64(ptr %arg_slot, ptr %args_ptr, i64 %tp_sz, i1 false)\n\
                     \x20 br label %do_call\n\
                     \n\
                     do_call:\n\
                     \x20 %ret_val = call {tr_llvm} @{entry_mangled}(ptr %arg_slot)\n\
                     \x20 %ret_slot = alloca {tr_llvm}\n\
                     \x20 store {tr_llvm} %ret_val, ptr %ret_slot\n\
                     \x20 %heap_ret = call ptr @malloc(i64 %tr_sz)\n\
                     \x20 call void @llvm.memcpy.p0.p0.i64(ptr %heap_ret, ptr %ret_slot, i64 %tr_sz, i1 false)\n\
                     \x20 ret ptr %heap_ret\n\
                     }}\n\n",
                ));
            }
        }

        out
    }

    /// Create a DISubprogram metadata node for a function (RFC 031 §2).
    ///
    /// Returns the metadata node ID to attach via `!dbg !N`. When debug info
    /// is disabled, returns 0 (no metadata attached).
    fn create_subprogram(&mut self, name: &str, _body: &MirCfgBody) -> u32 {
        if !self.dbg.enabled() {
            return 0;
        }
        // Use a void subroutine type as the generic signature — individual
        // parameter types are not yet tracked in MIR's DISubprogram. This is
        // sufficient for lldb to resolve function names and set breakpoints.
        let ret_type_id = None; // void return placeholder
        let subroutine_type_id = self.dbg.add_subroutine_type(ret_type_id, &[]);
        let linkage_name = mangle_fn_name(name);
        // Line 0 = unknown (MIR doesn't carry the function's definition span yet).
        // DILocation for individual instructions may carry accurate line info.
        self.dbg
            .add_subprogram(name, &linkage_name, 0, subroutine_type_id)
    }

    fn emit_struct_types(&self) -> String {
        let mut out = String::new();
        // ArcHeader: { refcount, vtable }
        out.push_str("%struct.ArcHeader = type { i32, ptr }\n");
        // RFC 008: arc_closure — runtime closure representation { fn_ptr, env_ptr }.
        out.push_str("%arc_closure = type { ptr, ptr }\n");

        // Struct types (value types)
        for (name, layout) in &self.layouts.structs {
            let field_tys: Vec<String> = layout
                .fields
                .iter()
                .map(|f| types::llvm_field_type(&f.ty, self.layouts))
                .collect();
            out.push_str(&format!(
                "%struct.{name} = type {{ {} }}\n",
                field_tys.join(", ")
            ));
        }

        // Class types (pointer types, but emit struct definition)
        for (name, layout) in &self.layouts.classes {
            let mut field_tys: Vec<String> = vec!["%struct.ArcHeader".into()];
            for f in &layout.fields {
                field_tys.push(types::llvm_field_type(&f.ty, self.layouts));
            }
            // RFC 037 M-D0：`[Observable]` auto-property 合成隐藏通知通道字段
            // （`ptr`，Signal<T> 惰性挂载点）。**每属性一槽**：按该类规范序
            // （`ProgramLayouts::class_observable_properties`，属性名升序）追加
            // 等量 `ptr`，与 `observable_channel_offset` / `class_size` 三处共用
            // 同一规范序——错位则多属性类 GEP 与 calloc 尺寸不一致，运行期崩溃。
            let obs_count = self.layouts.class_observable_properties(name).len();
            for _ in 0..obs_count {
                field_tys.push("ptr".into());
            }
            out.push_str(&format!(
                "%struct.{name} = type {{ {} }}\n",
                field_tys.join(", ")
            ));
        }

        // RFC 004 M1：variant 类型定义
        //
        // 内存布局：`{ u8 tag, [3 x i8] pad, %variant.{Name}.body payload }`
        // - tag：case discriminant（u8，足够 256 个 case）
        // - pad：3 字节填充，使 payload 4 字节对齐（class payload 是 ptr，需 8 字节对齐）
        // - payload：所有有 payload 的 case 的类型组成的 LLVM union
        //
        // LLVM union 写法：`%variant.{Name}.body = type { <largest_payload_ty> }`
        // 选择最大的 payload 类型作为容器（其他较小类型通过 bitcast 写入/读取）。
        // 若所有 case 都无 payload，body 为 `{ [0 x i8] }`（零大小占位）。
        for (name, vlayout) in &self.layouts.variants {
            // 收集所有 case 的 payload LLVM 类型
            let mut payload_tys: Vec<String> = Vec::new();
            for case in &vlayout.cases {
                if let Some(p) = &case.payload {
                    let ty_id = TypeId::Named(p.clone());
                    payload_tys.push(types::llvm_type_of(&ty_id, self.layouts));
                }
            }
            // 选择最大的 payload 类型作为 union 容器（按字节大小降序选首个）
            // 简单启发式：ptr > double > i64 > i32；其他都视为 ptr（结构体/字符串句柄）
            let body_ty = if payload_tys.is_empty() {
                "[0 x i8]".to_string()
            } else {
                pick_largest_payload(&payload_tys)
            };
            out.push_str(&format!("%variant.{name}.body = type {{ {body_ty} }}\n"));
            out.push_str(&format!(
                "%variant.{name} = type {{ i8, [3 x i8], %variant.{name}.body }}\n"
            ));
        }

        out
    }

    fn emit_vtables(&mut self) -> String {
        let mut out = String::new();
        // RFC 018 M1: 先发射所有 typeinfo 全局常量，供 vtable slot 0 引用。
        // 仅对 has_vtable == true 的 class 发射（无 vtable 的 class 不参与
        // `is` 测试，因 rt_obj_isa 通过 vtable slot 0 读取 typeinfo）。
        out.push_str(&self.emit_typeinfos());

        for class in self.layouts.classes.values() {
            if !class.has_vtable {
                continue;
            }
            // 外部类（依赖包导出面）由定义包发射，本 TU 跳过（守卫 on-demand
            // 登记 external 声明）。
            if self.external_class_names.contains(class.name.as_str()) {
                continue;
            }
            let cname = class.name.as_str();
            // VTable 类型（RFC 018 D5 修订 / RFC 006 finalizer）：
            //   slot 0: const RtTypeInfo* typeinfo
            //   slot 1: finalizer fn ptr（RFC 006：class 字段释放；无 class 字段为 null）
            //   slot 2+: virtual methods
            let slot_count = class.virtual_slots.len() + 3;
            out.push_str(&format!(
                "%vtable.{cname} = type {{ {} }}\n",
                vec!["ptr"; slot_count].join(", ")
            ));
            // RFC 006 M2：为含 class 字段的 class 生成 `__finalize_{cname}`。
            // 遍历 class 类型字段（排除 opaque runtime handle），加载字段值
            // 并 `rt_arc_dec`。`rt_arc_dec` 归零时（M3）经 vtable slot 1 调用，
            // 统一释放嵌套 class 字段引用（解决 header-only drop 泄漏）。
            let class_field_slots: Vec<(String, u32)> = class
                .fields
                .iter()
                .filter(|f| {
                    self.layouts.classes.contains_key(f.ty.as_str())
                        && !is_opaque_runtime_handle(f.ty.as_str())
                })
                .map(|f| (f.ty.to_string(), f.offset))
                .collect();
            let has_class_fields = !class_field_slots.is_empty();
            if has_class_fields {
                let mut fin = String::new();
                fin.push_str(&format!("$__finalize_{cname} = comdat any\n"));
                fin.push_str(&format!(
                    "define linkonce_odr void @__finalize_{cname}(ptr %self) comdat {{\n"
                ));
                fin.push_str("entry:\n");
                for (ty, off) in &class_field_slots {
                    // object pointer = %self + offset（header 16B + 字段偏移）。
                    let fld = format!("%__finalize_{cname}_f{off}");
                    fin.push_str(&format!("{fld} = getelementptr i8, ptr %self, i64 {off}\n"));
                    let val = format!("%__finalize_{cname}_v{off}");
                    fin.push_str(&format!("{val} = load ptr, ptr {fld}\n"));
                    fin.push_str(&format!("call void @rt_arc_dec(ptr {val})\n"));
                    let _ = ty;
                }
                fin.push_str("ret void\n");
                fin.push_str("}\n");
                out.push_str(&fin);
                // RFC 005 M2：`__walk_{cname}`——对每个 class 类型字段调
                // `visit(ctx, field_obj)`，供循环收集器（试删）经
                // `rt_arc_walk_fields` 遍历对象字段。与 finalizer 同 filter
                // （class 类型、非 opaque handle）。step-1 死代码（收集器未
                // 实现），布局无条件发射（flag 无关）。
                let mut walk = String::new();
                walk.push_str(&format!("$__walk_{cname} = comdat any\n"));
                walk.push_str(&format!(
                    "define linkonce_odr void @__walk_{cname}(ptr %self, ptr %visit, ptr %ctx) comdat {{\n"
                ));
                walk.push_str("entry:\n");
                for (ty, off) in &class_field_slots {
                    let fld = format!("%__walk_{cname}_f{off}");
                    walk.push_str(&format!("{fld} = getelementptr i8, ptr %self, i64 {off}\n"));
                    let val = format!("%__walk_{cname}_v{off}");
                    walk.push_str(&format!("{val} = load ptr, ptr {fld}\n"));
                    walk.push_str(&format!("call void %visit(ptr %ctx, ptr {val})\n"));
                    let _ = ty;
                }
                walk.push_str("ret void\n");
                walk.push_str("}\n");
                out.push_str(&walk);
            }
            // VTable 实例（RFC 004：slot 0 typeinfo / slot 1 finalizer /
            // slot 2 walk / slot 3+ virtual methods）
            let mut fns = vec![
                format!("ptr @.typeinfo.{cname}"), // slot 0: typeinfo
                if has_class_fields {
                    format!("ptr @__finalize_{cname}") // slot 1: finalizer
                } else {
                    "ptr null".to_string() // slot 1: 无 class 字段 → null
                },
                if has_class_fields {
                    format!("ptr @__walk_{cname}") // slot 2: walk
                } else {
                    "ptr null".to_string() // slot 2: 无 class 字段 → null
                },
            ];
            for slot in &class.virtual_slots {
                // CD-10/D1：槽位实现（最派生 override）与链接名（含重载消歧后缀）
                // 已由 typeck 在 VirtualSlot 中解析，codegen 直接引用。
                let fn_name = mangle_fn_name(&slot.link_name);
                fns.push(format!("ptr @{fn_name}"));
            }
            // RFC 038 M2 链接模型 → **RFC 047 修订**：vtable 全局由定义包
            // 发射为**强定义常量**（原 linkonce_odr + COMDAT——实测无引用的
            // COMDAT 节在 clang COFF 落盘时被丢弃，而 RFC 047 迁移需要
            // `.vtable.{T}` 经 rt_library_sym 按 dll 解析——弱定义不可依赖）。
            // 插件 dll 为单 TU 编译，强定义无跨包重复风险；消费方的
            // external 引用（vtable_global 守卫登记）由定义包导出面
            // （all_exports 的 .vtable.{T}）解析。
            out.push_str(&format!(
                "@.vtable.{cname} = constant %vtable.{cname} {{ {} }}\n",
                fns.join(", ")
            ));
        }
        out
    }

    /// RFC 004 P0 Phase 2：发射 struct 装箱 vtable 全局常量。
    ///
    /// 每个 struct `T` 发射 `@.vtable.{T}_Box = [3 x ptr] [@.typeinfo.{T}, null, null]`，
    /// slot0 = `@.typeinfo.{T}`（`o is T` 判别；struct 无虚方法/finalizer，slot1/2 null）。
    /// 供 `emit_struct_box` 在 `rt_box_create` 后内联 store 到 ArcHeader.vtable。
    fn emit_boxed_struct_vtables(&self) -> String {
        let mut out = String::new();
        for sname in self.layouts.structs.keys() {
            // RFC 038 M2 链接模型：struct 装箱 vtable 由**定义包**发射为
            // `linkonce_odr` + COMDAT——外部 struct（`external_class_names`
            // 成员，任意角色）的 typeinfo 不在本 TU 发射（emit_typeinfos
            // 跳过），其 `@.vtable.{S}_Box` 引用点（emit_struct_box）经守卫
            // 登记为 external 声明，由定义包 linkonce_odr 定义解析。
            if self.external_class_names.contains(sname.as_str()) {
                continue;
            }
            out.push_str(&format!("$.vtable.{sname}_Box = comdat any\n"));
            out.push_str(&format!(
                "@.vtable.{sname}_Box = linkonce_odr constant [3 x ptr] \
                 [ptr @.typeinfo.{sname}, ptr null, ptr null], comdat\n"
            ));
        }
        out
    }

    /// 根据类型名返回其 typeinfo 全局符号，供反射元数据（RtFieldInfo.field_type /
    /// RtMethodInfo.return_type / RtPropertyInfo.property_type）填充类型指针。
    ///
    /// class/struct/interface/enum 引用 `emit_typeinfos` 发射的 `@.typeinfo.{Type}`
    /// 全局常量。基元类型（RFC 018 §5.2.2）由 runtime 静态初始化，RFC 017 阶段一
    /// 起数据符号 static 化不可跨映像引用——返回 `null` 初值，真实指针由
    /// `pending_prim_fills` 在 `__arc_module_init` 统一回填。
    ///
    /// 数组（`{Elem}_arr`）、函数指针 / 委托及其它 ctype 类型**没有** typeinfo 全局常量，
    /// 返回 `null`，避免生成对未定义符号 `@.typeinfo.{name}` 的引用（clang IR 编译失败）。
    ///
    /// RFC 038 M2 链接模型：外部（core Arc / 依赖包）类型——凡属
    /// `external_class_names`（任意角色）typeinfo 不在本 TU 发射，登记
    /// `external global` 声明（定义包 `.o` 的 linkonce_odr 定义解析，本 TU 只
    /// 取地址）；非外部类型本 TU 发射，直接引用。
    fn typeinfo_global_for(&mut self, type_name: &str) -> String {
        match typeinfo_symbol_core(self.layouts, type_name) {
            Some(sym) => {
                if self.needs_external_typeinfo_decl(type_name) {
                    self.external_aggregate_refs
                        .entry(sym.trim_start_matches('@').to_string())
                        .or_insert_with(|| RT_TYPEINFO_LLVM_TY.to_string());
                }
                sym
            }
            None => "null".to_string(),
        }
    }

    /// RFC 038 M2：外部类 typeinfo 是否需登记 external 声明——凡属
    /// `external_class_names`（依赖包导出面，`emit_typeinfos` 跳过本 TU 发射，
    /// 定义包 linkonce_odr 定义）即需登记，与角色无关。
    fn needs_external_typeinfo_decl(&self, type_name: &str) -> bool {
        self.external_class_names.contains(type_name)
    }

    /// RFC 038 M2 链接模型：class vtable 全局引用守卫（ModuleEmitter 侧，
    /// `__sinit` 静态初始化器路径；函数体路径见 `FnEmitter::vtable_global`）。
    ///
    /// 类无 vtable 返回 `None`（调用方跳过 vptr 槽写入）；外部类
    /// （`external_class_names` 成员，任意角色）登记 external 声明并返回
    /// 符号名——槽位数取自本 TU 布局（typeck 从外部符号表构建，与消费者
    /// 源码地基一致），仅供声明类型占位，不参与代码语义。
    fn vtable_global_reg(&mut self, class: &str) -> Option<String> {
        if !class_has_vtable(self.layouts, class) {
            return None;
        }
        let sym = format!("@.vtable.{class}");
        if self.external_class_names.contains(class) {
            let slots = self
                .layouts
                .classes
                .get(class)
                .map(|c| c.virtual_slots.len() + 3)
                .unwrap_or(3);
            self.external_aggregate_refs
                .entry(sym[1..].to_string())
                .or_insert_with(|| format!("[{slots} x ptr]"));
        }
        Some(sym)
    }

    /// RFC 038 M2 链接模型：发射本 TU 登记的外部类聚合全局声明
    /// （`@<sym> = external global <ty>`）。位置在模块末尾（LLVM 允许前向
    /// 引用）——登记贯穿 FnEmitter（函数体）与 `__sinit`（静态初始化器）
    /// 两个发射阶段，须待全部完成后统一输出。
    fn emit_external_aggregate_decls(&self) -> String {
        if self.external_aggregate_refs.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        out.push_str(
            "; ---- RFC 038 M2: external class vtable/typeinfo globals ----\n\
             ; defined by consumer .o (linkonce_odr constant); address-taken only.\n",
        );
        for (sym, ty) in &self.external_aggregate_refs {
            out.push_str(&format!("@{sym} = external global {ty}\n"));
        }
        out.push('\n');
        out
    }

    /// 返回沿 parent 链收集的**继承字段名集合**（不含本类自身声明）。
    /// `ClassLayout.fields` 为扁平化（含继承）字段列表；`declared_fields` 数组
    /// 须仅含本类型声明字段，故需排除继承字段（RFC 018 J-C DeclaredXxx 语义）。
    fn inherited_field_names(&self, class_name: &str) -> std::collections::HashSet<String> {
        let mut inherited = std::collections::HashSet::new();
        let mut cur = self
            .layouts
            .classes
            .get(class_name)
            .and_then(|c| c.parent.clone());
        while let Some(p) = cur {
            if let Some(pc) = self.layouts.classes.get(p.as_str()) {
                for f in &pc.fields {
                    inherited.insert(f.name.as_str().to_string());
                }
                cur = pc.parent.clone();
            } else {
                break;
            }
        }
        inherited
    }

    /// 接口是否有已发射的 itable。`emit_itables` 对 layouts 内登记的每个
    /// (class/struct, iface) 组合均发射 `@.itable.{...}`（标记接口为空槽表），
    /// 故此处仅排除 layout 缺失（外部接口不发射）的情形。
    fn iface_has_itable(&self, iface_name: &str) -> bool {
        self.layouts.interfaces.contains_key(iface_name)
    }

    /// RFC 018 M1 + RFC 018 M1 + RFC 017 M1: 发射 `@.typeinfo.{Type}` 全局常量。
    ///
    /// RFC 017 M1 扩展：
    /// - **接口 typeinfo**：为每个 interface 发射 `@.typeinfo.{Iname}`（kind=3，
    ///   RT_TYPE_KIND_INTERFACE），供 `rt_obj_isa` 接口遍历使用
    /// - **implemented_interfaces 数组**：为实现了接口的 class 生成
    ///   `@.typeinfo.impl_ifaces.{Class}` 指针数组，填入 class typeinfo
    ///   的 `implemented_interfaces` 字段
    ///
    /// 每个 class 对应一个 RtTypeInfo 结构（**不限 has_vtable**——无 vtable 的
    /// 纯数据类同样需要 `typeof(T).TypeId` / DI 类型身份；2026-07-31 修复）。
    /// RFC 018 M1 仅含
    /// `{ i32 type_id, ptr parent }`；RFC 018 M1 扩展为完整 24 字段结构
    /// （对齐 rt_abi.h RtTypeInfo 定义）：
    ///
    /// ```text
    /// { i32 type_id, ptr parent, ptr name, ptr full_name, ptr ns,
    ///   i32 kind, i32 flags,
    ///   ptr declared_methods, i32 declared_method_count,
    ///   ptr declared_fields,  i32 declared_field_count,
    ///   ptr declared_properties, i32 declared_property_count,
    ///   ptr declared_events, i32 declared_event_count,
    ///   ptr declared_constructors, i32 declared_ctor_count,
    ///   ptr implemented_interfaces, i32 interface_count,
    ///   ptr element_type,
    ///   ptr declared_nested_types, i32 nested_type_count,
    ///   ptr attributes, i32 attribute_count }
    /// ```
    ///
    /// **M1 范围**：name 为类型键字符串；full_name/ns 发射真实点分限定名
    /// （RFC 018 M2：layout 层 `type_full_names` 由 HIR namespace 经
    /// `type_fqn` 拼接）；kind = CLASS(1)；
    /// flags = 0；所有 declared_* 数组为 null + count=0（M2 起逐步填充）；
    /// RFC 017 M1：implemented_interfaces 已填充（从 class 的接口列表派生）；
    /// element_type/declared_nested_types/attributes 均为 null + count=0。
    ///
    /// - type_id：复用 RFC 026 `typeof(T)` 的 FNV-1a hash（type_name_to_id）
    /// - parent：直接基类的 typeinfo 指针；基类无 vtable 或无基类时为 null
    ///
    /// **物理边界**（RFC 018 §3.3）：结构体不含函数指针/字段偏移，从 ABI
    /// 物理层面杜绝 Invoke/GetValue/SetValue。
    fn emit_typeinfos(&mut self) -> String {
        let mut out = String::new();
        // RtTypeInfo LLVM 类型：25 字段（对齐 rt_abi.h RtTypeInfo 定义；
        // 末字段 interface_itables 为 RFC 004 P0 后续 Sprint 追加，供 rt_obj_to_iface）
        let ti_ty = "{ i32, ptr, ptr, ptr, ptr, i32, i32, \
                     ptr, i32, ptr, i32, ptr, i32, ptr, i32, ptr, i32, \
                     ptr, i32, ptr, ptr, i32, ptr, i32, ptr }";
        // RFC 038 M2 链接模型：类型元数据（typeinfo）由**定义包**发射为
        // `linkonce_odr` + COMDAT——外部类型（external_class_names，即依赖包导出
        // 面的成员）在本 TU 不发射，引用点经 `typeinfo_global` 守卫登记
        // `@.typeinfo.{T} = external global` 声明、由定义包的 linkonce_odr 定义
        // 解析（与库函数符号同构：定义包提供、消费方外部引用）。此模型保证
        // 元数据完整（含 implemented_interfaces/itable，消费方无 .aopkg 关系面
        // 无法重建）且全程序单一定义（每类型仅定义包发射）。
        // 不捕获 self（避免与守卫的 &mut 借用冲突）。返回 (linkage, comdat 后缀)
        // ——`linkonce_odr` 定义行须以 `, comdat` 收尾（COFF 上 lld 才将其折叠
        // 为 COMDAT group）。
        let typeinfo_linkage = |out: &mut String, name: &str| -> (&'static str, &'static str) {
            out.push_str(&format!("$.typeinfo.{name} = comdat any\n"));
            ("linkonce_odr", ", comdat")
        };

        // RFC 018 M2: RtFieldInfo 命名类型（供 declared_fields 数组使用）
        let mut rt_field_info_declared = false;
        // RFC 018 M2: RtMethodInfo 命名类型（供 declared_methods 数组使用）
        let mut rt_method_info_declared = false;
        // RFC 018 M3+: RtPropertyInfo 命名类型（供 declared_properties 数组使用）
        let mut rt_property_info_declared = false;

        // RFC 023 冲刺批次一：type_id 唯一性守卫。下方 interface/class/struct/
        // enum 四循环覆盖本 TU 全部会获得 type_id 的类型名（与各循环
        // type_name_to_id 计算点同位登记）；基元 typeinfo（@rt_typeinfo_<prim>）
        // 由 runtime 静态初始化且其名为语言关键字，用户类型不可达，不在收集
        // 范围。异名同哈希即编译期拒绝（对齐 RFC 016 M2 编译期拒绝惯例）。
        let mut type_id_guard = crate::llvm_ir::emit_rvalue::TypeIdUniquenessGuard::new();

        // RFC 018 M2：发射 name/full_name/ns 三常量。name 保持类型键（type_id
        // 哈希输入与全 TU 符号寻址不变——RFC 026 `type_name_to_id` 勿动共识）；
        // full_name/ns 由 layout 层 `type_full_names`（HIR namespace 经
        // `type_fqn` 拼接）提供真实点分限定名，键缺失回退键名。返回三个
        // RtTypeInfo 字段指针（name / full_name / ns）。
        let typeinfo_name_fields = |out: &mut String, key: &str| -> (String, String, String) {
            let full_name = self
                .layouts
                .type_full_names
                .get(key)
                .map(|s| s.as_str())
                .unwrap_or(key);
            let ns_part = full_name.rsplit_once('.').map(|(ns, _)| ns).unwrap_or("");
            out.push_str(&format!(
                "@.typeinfo.name.{key} = private unnamed_addr constant [{len} x i8] c\"{key}\\00\"\n",
                len = key.len() + 1,
            ));
            out.push_str(&format!(
                "@.typeinfo.fullname.{key} = private unnamed_addr constant [{len} x i8] c\"{full_name}\\00\"\n",
                len = full_name.len() + 1,
            ));
            out.push_str(&format!(
                "@.typeinfo.ns.{key} = private unnamed_addr constant [{len} x i8] c\"{ns_part}\\00\"\n",
                len = ns_part.len() + 1,
            ));
            (
                format!("ptr @.typeinfo.name.{key}"),
                format!("ptr @.typeinfo.fullname.{key}"),
                format!("ptr @.typeinfo.ns.{key}"),
            )
        };

        // RFC 017 M1: 先发射接口 typeinfo，供 class `implemented_interfaces` 引用
        for (iname, _ilayout) in &self.layouts.interfaces {
            // 外部接口（依赖包导出面）由定义包发射，本 TU 跳过（守卫 on-demand
            // 登记 external 声明）。
            if self.external_class_names.contains(iname.as_str()) {
                continue;
            }
            let type_id = crate::llvm_ir::emit_rvalue::type_name_to_id(iname.as_str());
            if let Err(collision) = type_id_guard.register(iname.as_str()) {
                panic!("codegen: {}", collision.render());
            }
            let (name_field, fullname_field, ns_field) =
                typeinfo_name_fields(&mut out, iname.as_str());
            // kind = 3 (RT_TYPE_KIND_INTERFACE)；parent = null；所有数组为空
            let (linkage, comdat) = typeinfo_linkage(&mut out, iname);
            out.push_str(&format!(
                "@.typeinfo.{iname} = {linkage} constant {ti_ty} {{\n\
                 \x20 i32 {type_id},\n\
                 \x20 ptr null,\n\
                 \x20 {name_field},\n\
                 \x20 {fullname_field},\n\
                 \x20 {ns_field},\n\
                 \x20 i32 3,\n\
                 \x20 i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null\n\
                 }}{comdat}\n",
            ));
        }

        for class in self.layouts.classes.values() {
            // 外部类（依赖包导出面）由定义包发射，本 TU 跳过（守卫 on-demand
            // 登记 external 声明）。
            if self.external_class_names.contains(class.name.as_str()) {
                continue;
            }
            let cname = class.name.as_str();
            let type_id = crate::llvm_ir::emit_rvalue::type_name_to_id(cname);
            if let Err(collision) = type_id_guard.register(cname) {
                panic!("codegen: {}", collision.render());
            }
            let parent_field = match &class.parent {
                // 仅当父类在本 TU 内会发射 typeinfo 时引用其 `@.typeinfo.{p}`；
                // 外部（core Arc / 依赖包）父类的 typeinfo 不在本 TU 发射
                // （按 external_class_names 过滤），引用会悬空
                // → 用 `ptr null`，避免 clang IR 编译失败（undefined @.typeinfo.{p}）。
                Some(p)
                    if self.layouts.classes.contains_key(p.as_str())
                        && !self.external_class_names.contains(p.as_str()) =>
                {
                    format!("ptr @.typeinfo.{p}")
                }
                _ => "ptr null".to_string(),
            };

            // RFC 017 M1: 为实现了接口的 class 生成 implemented_interfaces 数组
            let (impl_ifaces_field, impl_ifaces_count) = if class.interfaces.is_empty() {
                ("ptr null".to_string(), "i32 0".to_string())
            } else {
                let array_name = format!("@.typeinfo.impl_ifaces.{cname}");
                let entries: Vec<String> = class
                    .interfaces
                    .iter()
                    .filter(|i| {
                        self.layouts.interfaces.contains_key(i.as_str())
                                // 外部接口 typeinfo 不在本 TU 发射 → 跳过，避免悬空引用。
                                && !self.external_class_names.contains(i.as_str())
                    })
                    .map(|i| format!("ptr @.typeinfo.{i}"))
                    .collect();
                let count = entries.len();
                if count > 0 {
                    let array_ty = format!("[{count} x ptr]");
                    let entries_str = entries.join(", ");
                    out.push_str(&format!(
                        "{array_name} = private constant {array_ty} [{entries_str}]\n"
                    ));
                    (format!("ptr {array_name}"), format!("i32 {count}"))
                } else {
                    ("ptr null".to_string(), "i32 0".to_string())
                }
            };

            // RFC 004 P0 后续 Sprint：与 implemented_interfaces 同索引平行的
            // interface_itables——每项为该接口的 itable 符号（class 视图）。
            // 标记接口无 itable → 平行槽 null（is 判别仍走 implemented_interfaces）。
            let impl_itables_field = if class.interfaces.is_empty() {
                "ptr null".to_string()
            } else {
                let array_name = format!("@.typeinfo.impl_itables.{cname}");
                let entries: Vec<String> = class
                    .interfaces
                    .iter()
                    .filter(|i| {
                        self.layouts.interfaces.contains_key(i.as_str())
                            && !self.external_class_names.contains(i.as_str())
                    })
                    .map(|i| {
                        if self.iface_has_itable(i.as_str()) {
                            format!("ptr @.itable.{cname}_{i}")
                        } else {
                            "ptr null".to_string()
                        }
                    })
                    .collect();
                let count = entries.len();
                if count > 0 {
                    let array_ty = format!("[{count} x ptr]");
                    let entries_str = entries.join(", ");
                    out.push_str(&format!(
                        "{array_name} = private constant {array_ty} [{entries_str}]\n"
                    ));
                    format!("ptr {array_name}")
                } else {
                    "ptr null".to_string()
                }
            };

            // RFC 018 M2：name/full_name/ns 三常量（full_name/ns 真实限定名）
            let (name_field, fullname_field, ns_field) = typeinfo_name_fields(&mut out, cname);

            // RFC 018 M2: 填充 declared_fields 数组（RtFieldInfo rodata）。
            // 从 ClassLayout.fields 派生，每个字段一条 RtFieldInfo 记录。
            // ClassLayout.fields 为扁平化（含继承）列表；RFC 018 J-C 语义下
            // declared_fields 仅本类型声明字段，故过滤掉继承字段。
            let inherited_names = self.inherited_field_names(cname);
            let declared_fields: Vec<_> = class
                .fields
                .iter()
                .filter(|f| !inherited_names.contains(f.name.as_str()))
                .collect();
            let (decl_fields_ptr, decl_fields_count) = if declared_fields.is_empty() {
                ("ptr null".to_string(), "i32 0".to_string())
            } else {
                // 延迟声明 %RtFieldInfo 命名类型（每个模块仅一次）
                if !rt_field_info_declared {
                    out.push_str("%RtFieldInfo = type { ptr, ptr, ptr, i32, ptr, i32 }\n");
                    rt_field_info_declared = true;
                }
                let fields_array_name = format!("@.typeinfo.fields.{cname}");
                let count = declared_fields.len();
                let mut prim_fills: Vec<(usize, i32)> = Vec::new();
                let entries: Vec<String> = declared_fields
                    .iter()
                    .enumerate()
                    .map(|(elem_idx, f)| {
                        let fname_global = format!("@__typeinfo_fname_{cname}_{fname}",
                            fname = f.name.as_str());
                        out.push_str(&format!(
                            "{fname_global} = private unnamed_addr constant [{len} x i8] c\"{fname_str}\\00\"\n",
                            len = f.name.as_str().len() + 1,
                            fname_str = f.name.as_str(),
                        ));
                        if let Some(prim_id) = primitive_typeinfo_id(f.ty.as_str()) {
                            prim_fills.push((elem_idx, prim_id));
                        }
                        format!(
                            "%RtFieldInfo {{ ptr {fname_global}, ptr @.typeinfo.{cname}, \
                             ptr {field_type_global}, i32 0, ptr null, i32 0 }}",
                            field_type_global = self.typeinfo_global_for(f.ty.as_str()),
                        )
                    })
                    .collect();
                let entries_str = entries.join(",\n  ");
                // RFC 017 阶段一：基元 typeinfo 经 rt_typeinfo_prim(id) 运行期查询，
                // 含基元槽的数组须可写（运行时在 __arc_module_init 回填真实指针），
                // 基元槽初值 null（typeinfo_global_for 对基元返回 null）。
                let linkage = if prim_fills.is_empty() { "constant" } else { "global" };
                out.push_str(&format!(
                    "{fields_array_name} = private {linkage} [{count} x %RtFieldInfo] [\n  {entries_str}\n]\n"
                ));
                for (elem_idx, prim_id) in prim_fills {
                    self.pending_prim_fills.push((
                        format!("getelementptr inbounds ([{count} x %RtFieldInfo], ptr {fields_array_name}, i64 0, i64 {elem_idx}, i32 2)"),
                        prim_id,
                    ));
                }
                (format!("ptr {fields_array_name}"), format!("i32 {count}"))
            };

            // RFC 018 M2: 填充 declared_methods 数组（RtMethodInfo rodata）。
            // 从 ClassLayout.declared_methods 派生。
            // parameters/return_type/attributes 暂为 null（M3 补齐）。
            //
            // 重载方法去重：C# 允许同名方法重载（如 `SaveChangesAsync()` 与
            // `SaveChangesAsync(CancellationToken)`），但 `@__typeinfo_mname_<C>_<M>`
            // 全局字符串常量按 (class, method_name) 命名——重载共享同一全局，
            // 二次声明会触发 LLVM IR redefinition 错误。用 HashSet 跟踪已发射的
            // mname_global，仅在首次出现时 emit 定义，重载复用同一指针。
            let (decl_methods_ptr, decl_methods_count) = if class.declared_methods.is_empty() {
                ("ptr null".to_string(), "i32 0".to_string())
            } else {
                // 延迟声明 %RtMethodInfo 命名类型（每个模块仅一次）
                if !rt_method_info_declared {
                    out.push_str(
                        "%RtMethodInfo = type { ptr, ptr, ptr, ptr, i32, i32, ptr, i32 }\n",
                    );
                    rt_method_info_declared = true;
                }
                let methods_array_name = format!("@.typeinfo.methods.{cname}");
                let count = class.declared_methods.len();
                let mut emitted_mnames: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut prim_fills: Vec<(usize, i32)> = Vec::new();
                let entries: Vec<String> = class
                    .declared_methods
                    .iter()
                    .enumerate()
                    .map(|(elem_idx, m)| {
                        let mname_global = format!("@__typeinfo_mname_{cname}_{mname}",
                            mname = m.name.as_str());
                        if emitted_mnames.insert(mname_global.clone()) {
                            out.push_str(&format!(
                                "{mname_global} = private unnamed_addr constant [{len} x i8] c\"{mname_str}\\00\"\n",
                                len = m.name.as_str().len() + 1,
                                mname_str = m.name.as_str(),
                            ));
                        }
                        if let Some(prim_id) = primitive_typeinfo_id(m.return_type.as_str()) {
                            prim_fills.push((elem_idx, prim_id));
                        }
                        format!(
                            "%RtMethodInfo {{ ptr {mname_global}, ptr @.typeinfo.{cname}, \
                             ptr {return_type_global}, ptr null, i32 {pcount}, i32 0, ptr null, i32 0 }}",
                            pcount = m.param_count,
                            return_type_global = self.typeinfo_global_for(m.return_type.as_str()),
                        )
                    })
                    .collect();
                let entries_str = entries.join(",\n  ");
                // RFC 017 阶段一：基元 typeinfo 经 rt_typeinfo_prim(id) 运行期查询，
                // 含基元槽的数组须可写（运行时在 __arc_module_init 回填真实指针），
                // 基元槽初值 null（typeinfo_global_for 对基元返回 null）。
                let linkage = if prim_fills.is_empty() { "constant" } else { "global" };
                out.push_str(&format!(
                    "{methods_array_name} = private {linkage} [{count} x %RtMethodInfo] [\n  {entries_str}\n]\n"
                ));
                for (elem_idx, prim_id) in prim_fills {
                    self.pending_prim_fills.push((
                        format!("getelementptr inbounds ([{count} x %RtMethodInfo], ptr {methods_array_name}, i64 0, i64 {elem_idx}, i32 2)"),
                        prim_id,
                    ));
                }
                (format!("ptr {methods_array_name}"), format!("i32 {count}"))
            };

            // RFC 018 M3+: 填充 declared_properties 数组（RtPropertyInfo rodata）。
            // property_type / get_method / set_method / attributes 暂为 null（后续补齐）。
            let (decl_props_ptr, decl_props_count) = if class.declared_properties.is_empty() {
                ("ptr null".to_string(), "i32 0".to_string())
            } else {
                if !rt_property_info_declared {
                    // { name, declaring, property_type, can_read, can_write,
                    //   get_method, set_method, flags, attributes, attribute_count }
                    out.push_str(
                        "%RtPropertyInfo = type { ptr, ptr, ptr, i32, i32, ptr, ptr, i32, ptr, i32 }\n",
                    );
                    rt_property_info_declared = true;
                }
                let props_array_name = format!("@.typeinfo.properties.{cname}");
                let count = class.declared_properties.len();
                let mut prim_fills: Vec<(usize, i32)> = Vec::new();
                let entries: Vec<String> = class
                    .declared_properties
                    .iter()
                    .enumerate()
                    .map(|(elem_idx, p)| {
                        let pname_global = format!(
                            "@__typeinfo_pname_{cname}_{pname}",
                            pname = p.name.as_str()
                        );
                        out.push_str(&format!(
                            "{pname_global} = private unnamed_addr constant [{len} x i8] c\"{pname_str}\\00\"\n",
                            len = p.name.as_str().len() + 1,
                            pname_str = p.name.as_str(),
                        ));
                        if let Some(prim_id) = primitive_typeinfo_id(p.property_type.as_str()) {
                            prim_fills.push((elem_idx, prim_id));
                        }
                        let can_read = if p.can_read { 1 } else { 0 };
                        let can_write = if p.can_write { 1 } else { 0 };
                        format!(
                            "%RtPropertyInfo {{ ptr {pname_global}, ptr @.typeinfo.{cname}, \
                             ptr {property_type_global}, i32 {can_read}, i32 {can_write}, ptr null, ptr null, \
                             i32 0, ptr null, i32 0 }}",
                            property_type_global = self.typeinfo_global_for(p.property_type.as_str()),
                        )
                    })
                    .collect();
                let entries_str = entries.join(",\n  ");
                // RFC 017 阶段一：基元 typeinfo 经 rt_typeinfo_prim(id) 运行期查询，
                // 含基元槽的数组须可写（运行时在 __arc_module_init 回填真实指针），
                // 基元槽初值 null（typeinfo_global_for 对基元返回 null）。
                let linkage = if prim_fills.is_empty() { "constant" } else { "global" };
                out.push_str(&format!(
                    "{props_array_name} = private {linkage} [{count} x %RtPropertyInfo] [\n  {entries_str}\n]\n"
                ));
                for (elem_idx, prim_id) in prim_fills {
                    self.pending_prim_fills.push((
                        format!("getelementptr inbounds ([{count} x %RtPropertyInfo], ptr {props_array_name}, i64 0, i64 {elem_idx}, i32 2)"),
                        prim_id,
                    ));
                }
                (format!("ptr {props_array_name}"), format!("i32 {count}"))
            };

            // RtTypeInfo 完整字段（24 项）
            // kind = 1 (RT_TYPE_KIND_CLASS)；flags = 0
            let (linkage, comdat) = typeinfo_linkage(&mut out, cname);
            out.push_str(&format!(
                "@.typeinfo.{cname} = {linkage} constant {ti_ty} {{\n\
                 \x20 i32 {type_id},\n\
                 \x20 {parent_field},\n\
                 \x20 {name_field},\n\
                 \x20 {fullname_field},\n\
                 \x20 {ns_field},\n\
                 \x20 i32 1,\n\
                 \x20 i32 0,\n\
                 \x20 {decl_methods_ptr}, {decl_methods_count},\n\
                 \x20 {decl_fields_ptr}, {decl_fields_count},\n\
                 \x20 {decl_props_ptr}, {decl_props_count},\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 {impl_ifaces_field}, {impl_ifaces_count},\n\
                 \x20 ptr null,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 {impl_itables_field}\n\
                 }}{comdat}\n",
            ));
        }

        // RFC 018 M1: 为 struct 发射完整 25 字段 typeinfo
        // kind = 2 (RT_TYPE_KIND_STRUCT)，无 parent；declared_* 数组为 null。
        // RFC 004 P0 后续 Sprint：填充 implemented_interfaces（接口 typeinfo）
        // 与 interface_itables（@.itable.{Struct}_Box_{Iface}），使 rt_obj_isa
        // 可判别 `o is IShape`、rt_obj_to_iface 可动态 downcast `(IShape)o`。
        for (sname, slayout) in &self.layouts.structs {
            // RFC 038 M2 链接模型：外部 struct（依赖包导出面）由定义包发射，
            // 本 TU 跳过（守卫 on-demand 登记 external 声明）。
            if self.external_class_names.contains(sname.as_str()) {
                continue;
            }
            let type_id = crate::llvm_ir::emit_rvalue::type_name_to_id(sname.as_str());
            if let Err(collision) = type_id_guard.register(sname.as_str()) {
                panic!("codegen: {}", collision.render());
            }
            let (name_field, fullname_field, ns_field) =
                typeinfo_name_fields(&mut out, sname.as_str());

            // 与 class 分支同构：implemented_interfaces（typeinfo 数组）与
            // interface_itables（平行 itable 数组），标记接口槽位为 null。
            let (impl_ifaces_field, impl_ifaces_count, impl_itables_field) =
                if slayout.interfaces.is_empty() {
                    (
                        "ptr null".to_string(),
                        "i32 0".to_string(),
                        "ptr null".to_string(),
                    )
                } else {
                    let ifaces: Vec<&Ident> = slayout
                        .interfaces
                        .iter()
                        .filter(|i| {
                            self.layouts.interfaces.contains_key(i.as_str())
                                && !self.external_class_names.contains(i.as_str())
                        })
                        .collect();
                    let count = ifaces.len();
                    if count > 0 {
                        let array_ty = format!("[{count} x ptr]");
                        let ifaces_array_name = format!("@.typeinfo.impl_ifaces.{sname}");
                        let itables_array_name = format!("@.typeinfo.impl_itables.{sname}");
                        let iface_entries = ifaces
                            .iter()
                            .map(|i| format!("ptr @.typeinfo.{i}"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let itable_entries = ifaces
                            .iter()
                            .map(|i| {
                                if self.iface_has_itable(i.as_str()) {
                                    format!("ptr @.itable.{sname}_Box_{i}")
                                } else {
                                    "ptr null".to_string()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        out.push_str(&format!(
                            "{ifaces_array_name} = private constant {array_ty} [{iface_entries}]\n"
                        ));
                        out.push_str(&format!(
                        "{itables_array_name} = private constant {array_ty} [{itable_entries}]\n"
                    ));
                        (
                            format!("ptr {ifaces_array_name}"),
                            format!("i32 {count}"),
                            format!("ptr {itables_array_name}"),
                        )
                    } else {
                        (
                            "ptr null".to_string(),
                            "i32 0".to_string(),
                            "ptr null".to_string(),
                        )
                    }
                };

            let (linkage, comdat) = typeinfo_linkage(&mut out, sname);
            out.push_str(&format!(
                "@.typeinfo.{sname} = {linkage} constant {ti_ty} {{\n\
                 \x20 i32 {type_id},\n\
                 \x20 ptr null,\n\
                 \x20 {name_field},\n\
                 \x20 {fullname_field},\n\
                 \x20 {ns_field},\n\
                 \x20 i32 2,\n\
                 \x20 i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 {impl_ifaces_field}, {impl_ifaces_count},\n\
                 \x20 ptr null,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 {impl_itables_field}\n\
                 }}{comdat}\n",
            ));
        }

        // RFC 018 M1: 为 enum 发射完整 24 字段 typeinfo
        // kind = 4 (RT_TYPE_KIND_ENUM)，无 parent，无 interface
        // M1 阶段 declared_* 数组全部为 null + count=0
        for ename in &self.layouts.enums {
            // RFC 038 M2 链接模型：外部 enum（依赖包导出面）由定义包发射，
            // 本 TU 跳过（守卫 on-demand 登记 external 声明）。
            if self.external_class_names.contains(ename.as_str()) {
                continue;
            }
            let type_id = crate::llvm_ir::emit_rvalue::type_name_to_id(ename.as_str());
            if let Err(collision) = type_id_guard.register(ename.as_str()) {
                panic!("codegen: {}", collision.render());
            }
            let (name_field, fullname_field, ns_field) =
                typeinfo_name_fields(&mut out, ename.as_str());
            let (linkage, comdat) = typeinfo_linkage(&mut out, ename);
            out.push_str(&format!(
                "@.typeinfo.{ename} = {linkage} constant {ti_ty} {{\n\
                 \x20 i32 {type_id},\n\
                 \x20 ptr null,\n\
                 \x20 {name_field},\n\
                 \x20 {fullname_field},\n\
                 \x20 {ns_field},\n\
                 \x20 i32 4,\n\
                 \x20 i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null, i32 0,\n\
                 \x20 ptr null\n\
                 }}{comdat}\n",
            ));
        }

        out
    }

    /// Emit interface itable globals: `@.itable.{class}_{iface}`.
    ///
    /// Method slots point at the concrete implementor, or at an **adapter thunk**
    /// when the interface view expects a different return ABI（RFC 009 P1-C2.6）：
    /// - concrete returns class、接口期望接口 → thunk 内 `MakeIface`
    /// - concrete returns 接口 A、视图期望接口 B（variance）→ thunk 内 itable 重绑定
    /// - 形参原样转发（`out T` 不在输入位；带参与零参同一路径）
    fn emit_itables(&self, fn_names: &HashSet<String>) -> String {
        let mut out = String::new();
        // 已合成接口属性 getter（`@<C>_get_<P>`）。auto-property / 初值属性不产生
        // 独立真实函数（访问直接字段加载），故不在 `fn_names` 中；当多个接口
        // 声明同一属性（如 `IRedirectResult : IWebResult` 均含 StatusCode）时，
        // 每个 itable 槽位都会触发同名的合成 getter，若不按名去重将生成重复定义。
        // 联合真实函数集合一起判定：真实已存在（custom getter）即跳过合成；
        // 否则每 (类, 属性) 只合成一次，多个 itable 共享该符号。
        let mut emitted_getters: HashSet<String> = HashSet::new();
        for class in self.layouts.classes.values() {
            let cname = class.name.as_str();
            for iface_name in &class.interfaces {
                let ilayout = match self.layouts.interfaces.get(iface_name) {
                    Some(il) => il,
                    None => continue,
                };
                // 标记接口（无任何成员）同样发射空槽表：MakeIface 对任意
                // class→iface 转换无条件引用 `@.itable.{C}_{I}`，空表仅作视图
                // 标记；空接口无方法槽位，不存在经空表分派的调用点。
                let mut fns: Vec<String> = Vec::new();
                for (mname, iface_ret, iface_params) in &ilayout.methods {
                    // CD-11/D2：按签名（名+形参）解析该接口方法在此类中的最终实现。
                    // method_impl 键含形参类型（重载各占其键），派生 override 覆盖
                    // 基类同签名条目 → itable 槽位命中派生类实现。
                    let Some((impl_class, concrete)) =
                        self.resolve_method_impl(class, mname.as_str(), iface_params)
                    else {
                        continue;
                    };
                    // RFC 019：接口可声明泛型方法（如 `IConfiguration.Get<T>`）。
                    // MIR `drop_non_emittable_generic_templates` 已剔除无法独立成
                    // 函数的泛型方法模板（函数体直接触碰泛型形参类型成员，产生
                    // 未解析的 `@T_*` 符号），但此类方法仍会出现在 layout 的
                    // interface method 列表里，其实现符号不在 `fn_names` 中。泛型
                    // 接口方法本就无法经 itable 分派（槽位无类型实参），故跳过该
                    // 槽位，避免引用未定义符号导致链接失败。
                    if !fn_names.contains(&concrete) {
                        continue;
                    }
                    if let Some(thunk) = self.emit_itable_adapter_thunk(
                        cname,
                        iface_name.as_str(),
                        mname.as_str(),
                        &impl_class,
                        iface_ret.as_str(),
                        iface_params,
                        &concrete,
                    ) {
                        out.push_str(&thunk);
                        let thunk_name =
                            format!("__ithunk_{cname}_{iface_name}_{}", mname.as_str());
                        fns.push(format!("ptr @{thunk_name}"));
                    } else {
                        fns.push(format!("ptr @{concrete}"));
                    }
                }
                for (pname, _) in &ilayout.properties {
                    // 属性 getter 同样沿 override 链解析（继承属性命中基类实现类）。
                    let getter_name = format!("get_{pname}");
                    let (impl_class, getter_fn) = self
                        .resolve_method_impl(class, &getter_name, &[])
                        .unwrap_or_else(|| (class.name.clone(), format!("{cname}_get_{pname}")));
                    fns.push(format!("ptr @{getter_fn}"));
                    if !fn_names.contains(&getter_fn) && emitted_getters.insert(getter_fn.clone()) {
                        out.push_str(&self.emit_iface_property_getter(
                            impl_class.as_str(),
                            pname,
                            &getter_fn,
                        ));
                    }
                }
                // RFC 006：泛型方法实例化槽位——每个实例化键（如 "Get__Seed"）
                // 指向该实现类的单态化实现 `C::Get__Seed`（mangle 为 `C_Get__Seed`）。
                // 槽位 ABI = 单态化签名，无适配器 thunk。
                for inst_name in &ilayout.generic_instances {
                    let mono_fn = mangle_method(cname, inst_name);
                    if fn_names.contains(&mono_fn) {
                        fns.push(format!("ptr @{mono_fn}"));
                    }
                }
                let slot_count = fns.len();
                out.push_str(&format!(
                    "@.itable.{cname}_{iface_name} = private constant [{slot_count} x ptr] [{}]\n",
                    fns.join(", ")
                ));
            }
        }

        // RFC 004 P0 Phase 2：struct 实现接口 → 发射 `@.itable.{Struct}_Box_{Iface}`。
        // struct 无 vtable，其接口分派须经 box（`object o = s; (I)o` 之外的静态类型
        // 赋值 `I i = s` 用固定 itable `MakeIface`）；槽位指向**值接收者 thunk**，
        // thunk 内把 `this` 从 box ptr 重定位到 `box+24`（byref 直调 `T::M(&boxed_value, …)`）。
        for s in self.layouts.structs.values() {
            let sname = s.name.as_str();
            for iface_name in &s.interfaces {
                let ilayout = match self.layouts.interfaces.get(iface_name) {
                    Some(il) => il,
                    None => continue,
                };
                // 标记接口与 class 路径同规则：空槽表照常发射（视图标记）。
                let mut fns: Vec<String> = Vec::new();
                for (mname, iface_ret, iface_params) in &ilayout.methods {
                    let Some(concrete) =
                        self.resolve_struct_method(s, mname.as_str(), iface_params)
                    else {
                        continue;
                    };
                    if !fn_names.contains(&concrete) {
                        continue;
                    }
                    let thunk_name = format!("__ithunk_{sname}_Box_{iface_name}_{mname}");
                    out.push_str(&self.emit_struct_value_thunk(
                        &thunk_name,
                        &concrete,
                        iface_ret.as_str(),
                        iface_params,
                    ));
                    fns.push(format!("ptr @{thunk_name}"));
                }
                let slot_count = fns.len();
                out.push_str(&format!(
                    "@.itable.{sname}_Box_{iface_name} = private constant [{slot_count} x ptr] [{}]\n",
                    fns.join(", ")
                ));
            }
        }
        out
    }

    /// RFC 004 P0 Phase 2：struct 接口方法在本 struct 的最终实现符号（签名键）。
    ///
    /// 与 `resolve_method_impl` 同构，但 struct 无父链——仅自身 `method_impl` 与
    /// `declared_methods`。精确签名键（名+形参）命中 → `declared_methods` 中同签名
    /// 条目的 `link_name`（含重载消歧后缀）；未命中按名回退。
    fn resolve_struct_method(
        &self,
        s: &StructLayout,
        method: &str,
        params: &[Ident],
    ) -> Option<String> {
        if let Some(impl_struct) = s.method_impl.get(&(method.into(), params.to_vec())) {
            if let Some(entry) = self
                .layouts
                .structs
                .get(impl_struct.as_str())
                .and_then(|st| {
                    st.declared_methods
                        .iter()
                        .find(|m| m.name.as_str() == method && m.param_types == params)
                })
            {
                return Some(mangle_fn_name(&entry.link_name));
            }
        }
        if let Some(entry) = s
            .declared_methods
            .iter()
            .find(|m| m.name.as_str() == method)
        {
            return Some(mangle_fn_name(&entry.link_name));
        }
        None
    }

    /// RFC 004 P0 Phase 2：struct 接口值接收者 thunk。
    ///
    /// struct 方法 ABI 的 `this` = 指向 struct 值的 ptr；接口分派传入的 `this` =
    /// box ptr。thunk 内把 `this` 从 box ptr 重定位到 `box+24`（payload 首地址），
    /// 再 byref 直调 `T::M(&boxed_value, …)`。与 class 接收者 thunk 的 `this` 约定
    /// 严格对齐（`this=box ptr`），杜绝跨「boxed struct / class」混调接口错位。
    fn emit_struct_value_thunk(
        &self,
        thunk_name: &str,
        concrete_fn: &str,
        ret: &str,
        params: &[Ident],
    ) -> String {
        let (sig, _) = self.thunk_param_forwarding(params);
        let mut call_args = vec!["ptr %val".to_string()];
        for (i, pty) in params.iter().enumerate() {
            let llvm = llvm_field_type(pty.as_str(), self.layouts);
            call_args.push(format!("{llvm} %arg{i}"));
        }
        let call_args_str = call_args.join(", ");
        if ret == "void" {
            format!(
                "define void @{thunk_name}({sig}) {{\n\
                 entry:\n\
                 \x20 %val = getelementptr inbounds i8, ptr %self, i64 24\n\
                 \x20 call void @{concrete_fn}({call_args_str})\n\
                 \x20 ret void\n\
                 }}\n\n"
            )
        } else {
            let ret_llvm = llvm_field_type(ret, self.layouts);
            format!(
                "define {ret_llvm} @{thunk_name}({sig}) {{\n\
                 entry:\n\
                 \x20 %val = getelementptr inbounds i8, ptr %self, i64 24\n\
                 \x20 %r = call {ret_llvm} @{concrete_fn}({call_args_str})\n\
                 \x20 ret {ret_llvm} %r\n\
                 }}\n\n"
            )
        }
    }

    /// Adapter thunk when the itable view ABI differs from the concrete method：
    /// - **协变返回**：class→iface wrap / iface→iface rebind
    /// - **逆变参数**：view 更窄的实参 → concrete 更宽的形参（class→iface wrap /
    ///   iface→iface rebind；class→class 同 ptr ABI 则原样转发）
    ///
    /// `out T` 不在输入位、`in T` 不在输出位，故两端不会同时改同一位置的类型名
    /// 却仍共用本 thunk（RFC 009 P1-C2.6）。
    fn emit_itable_adapter_thunk(
        &self,
        class: &str,
        iface: &str,
        method: &str,
        impl_class: &str,
        iface_ret: &str,
        iface_params: &[Ident],
        concrete_fn: &str,
    ) -> Option<String> {
        let concrete_ret = self
            .class_method_return_type(impl_class, method, iface_params)
            .unwrap_or_else(|| iface_ret.to_string());
        let concrete_params = self.class_method_param_types(impl_class, method, iface_params);
        let ret_adapt = is_iface_name(iface_ret) && concrete_ret != iface_ret;
        let param_adapt = iface_params.len() == concrete_params.len()
            && iface_params
                .iter()
                .zip(concrete_params.iter())
                .any(|(a, b)| a.as_str() != b.as_str());
        if !ret_adapt && !param_adapt {
            return None;
        }

        let thunk_name = format!("__ithunk_{class}_{iface}_{method}");
        let (sig_params, _view_call) = self.thunk_param_forwarding(iface_params);

        // Build call args: adapt each param from view ABI → concrete ABI.
        let mut pre = String::new();
        let mut call_args = vec!["ptr %self".to_string()];
        for (i, (view_ty, conc_ty)) in iface_params.iter().zip(concrete_params.iter()).enumerate() {
            let view_llvm = llvm_field_type(view_ty.as_str(), self.layouts);
            let conc_llvm = llvm_field_type(conc_ty.as_str(), self.layouts);
            if view_ty.as_str() == conc_ty.as_str() {
                call_args.push(format!("{view_llvm} %arg{i}"));
                continue;
            }
            // class/iface → iface：包装或重绑定为 concrete 期望的 fat pointer
            if is_iface_name(conc_ty.as_str()) {
                let adapted = format!("adapted{i}");
                pre.push_str(&self.emit_param_to_iface_adapt(
                    i,
                    view_ty.as_str(),
                    conc_ty.as_str(),
                    &adapted,
                )?);
                call_args.push(format!("ptr %{adapted}"));
            } else if view_llvm == conc_llvm {
                // class→class（继承）等同 ptr / 标量 ABI
                call_args.push(format!("{view_llvm} %arg{i}"));
            } else {
                return None;
            }
        }
        let call_args_str = call_args.join(", ");

        let ret_llvm = if iface_ret == "void" {
            "void".to_string()
        } else {
            llvm_field_type(iface_ret, self.layouts)
        };

        if !ret_adapt {
            // 仅参数适配（典型 `in T` / void 或不变返回）
            if ret_llvm == "void" {
                return Some(format!(
                    "define void @{thunk_name}({sig_params}) {{\n\
                     entry:\n\
                     {pre}\
                     \x20 call void @{concrete_fn}({call_args_str})\n\
                     \x20 ret void\n\
                     }}\n\n"
                ));
            }
            return Some(format!(
                "define {ret_llvm} @{thunk_name}({sig_params}) {{\n\
                 entry:\n\
                 {pre}\
                 \x20 %raw = call {ret_llvm} @{concrete_fn}({call_args_str})\n\
                 \x20 ret {ret_llvm} %raw\n\
                 }}\n\n"
            ));
        }

        // 返回适配（协变；可叠加参数转发）
        if self.layouts.classes.contains_key(concrete_ret.as_str()) {
            let provider = self
                .itable_provider_for(concrete_ret.as_str(), iface_ret)
                .unwrap_or_else(|| concrete_ret.clone());
            let itable_iface = self
                .layouts
                .classes
                .get(provider.as_str())
                .and_then(|pc| {
                    pc.interfaces
                        .iter()
                        .find(|i| {
                            let n = i.as_str();
                            n == iface_ret
                                || (iface_generic_root(n) == iface_generic_root(iface_ret)
                                    && n.contains('_')
                                    && iface_ret.contains('_'))
                        })
                        .map(|i| i.to_string())
                })
                .unwrap_or_else(|| iface_ret.to_string());
            return Some(format!(
                "define ptr @{thunk_name}({sig_params}) {{\n\
                 entry:\n\
                 {pre}\
                 \x20 %raw = call ptr @{concrete_fn}({call_args_str})\n\
                 \x20 %fat = alloca {{ ptr, ptr }}\n\
                 \x20 %oa = getelementptr inbounds {{ ptr, ptr }}, ptr %fat, i32 0, i32 0\n\
                 \x20 store ptr %raw, ptr %oa\n\
                 \x20 %va = getelementptr inbounds {{ ptr, ptr }}, ptr %fat, i32 0, i32 1\n\
                 \x20 store ptr @.itable.{provider}_{itable_iface}, ptr %va\n\
                 \x20 ret ptr %fat\n\
                 }}\n\n"
            ));
        }
        if is_iface_name(&concrete_ret) && concrete_ret != iface_ret {
            let pairs: Vec<(String, String)> = self
                .layouts
                .classes
                .values()
                .filter(|c| {
                    c.interfaces.iter().any(|i| i.as_str() == concrete_ret)
                        && c.interfaces.iter().any(|i| i.as_str() == iface_ret)
                })
                .map(|c| {
                    (
                        format!("@.itable.{}_{}", c.name, concrete_ret),
                        format!("@.itable.{}_{}", c.name, iface_ret),
                    )
                })
                .collect();
            let mut body = format!(
                "define ptr @{thunk_name}({sig_params}) {{\n\
                 entry:\n\
                 {pre}\
                 \x20 %src = call ptr @{concrete_fn}({call_args_str})\n\
                 \x20 %obj_a = getelementptr inbounds {{ ptr, ptr }}, ptr %src, i32 0, i32 0\n\
                 \x20 %obj = load ptr, ptr %obj_a\n\
                 \x20 %it_a = getelementptr inbounds {{ ptr, ptr }}, ptr %src, i32 0, i32 1\n\
                 \x20 %src_it = load ptr, ptr %it_a\n\
                 \x20 %fat = alloca {{ ptr, ptr }}\n\
                 \x20 %oa = getelementptr inbounds {{ ptr, ptr }}, ptr %fat, i32 0, i32 0\n\
                 \x20 store ptr %obj, ptr %oa\n\
                 \x20 %va = getelementptr inbounds {{ ptr, ptr }}, ptr %fat, i32 0, i32 1\n\
                 \x20 store ptr %src_it, ptr %va\n"
            );
            let join = "join";
            if pairs.is_empty() {
                body.push_str("  br label %join\n");
            } else {
                for (i, (from_it, to_it)) in pairs.iter().enumerate() {
                    let matched = format!("m{i}");
                    let next = if i + 1 < pairs.len() {
                        format!("n{i}")
                    } else {
                        join.to_string()
                    };
                    body.push_str(&format!(
                        "  %c{i} = icmp eq ptr %src_it, {from_it}\n\
                         \x20 br i1 %c{i}, label %{matched}, label %{next}\n\
                         {matched}:\n\
                         \x20 store ptr {to_it}, ptr %va\n\
                         \x20 br label %{join}\n"
                    ));
                    if i + 1 < pairs.len() {
                        body.push_str(&format!("{next}:\n"));
                    }
                }
            }
            body.push_str(&format!(
                "{join}:\n\
                 \x20 ret ptr %fat\n\
                 }}\n\n"
            ));
            return Some(body);
        }
        None
    }

    /// Emit IR that adapts view argument `%arg{i}` into iface fat pointer `{dest}`.
    fn emit_param_to_iface_adapt(
        &self,
        i: usize,
        view_ty: &str,
        iface_ty: &str,
        dest: &str,
    ) -> Option<String> {
        if is_iface_name(view_ty) {
            let pairs: Vec<(String, String)> = self
                .layouts
                .classes
                .values()
                .filter(|c| {
                    c.interfaces.iter().any(|n| n.as_str() == view_ty)
                        && c.interfaces.iter().any(|n| n.as_str() == iface_ty)
                })
                .map(|c| {
                    (
                        format!("@.itable.{}_{}", c.name, view_ty),
                        format!("@.itable.{}_{}", c.name, iface_ty),
                    )
                })
                .collect();
            let join = format!("{dest}_join");
            let mut s = format!(
                "  %{dest}_obj_a = getelementptr inbounds {{ ptr, ptr }}, ptr %arg{i}, i32 0, i32 0\n\
                 \x20 %{dest}_obj = load ptr, ptr %{dest}_obj_a\n\
                 \x20 %{dest}_it_a = getelementptr inbounds {{ ptr, ptr }}, ptr %arg{i}, i32 0, i32 1\n\
                 \x20 %{dest}_src_it = load ptr, ptr %{dest}_it_a\n\
                 \x20 %{dest} = alloca {{ ptr, ptr }}\n\
                 \x20 %{dest}_oa = getelementptr inbounds {{ ptr, ptr }}, ptr %{dest}, i32 0, i32 0\n\
                 \x20 store ptr %{dest}_obj, ptr %{dest}_oa\n\
                 \x20 %{dest}_va = getelementptr inbounds {{ ptr, ptr }}, ptr %{dest}, i32 0, i32 1\n\
                 \x20 store ptr %{dest}_src_it, ptr %{dest}_va\n"
            );
            if pairs.is_empty() {
                s.push_str(&format!("  br label %{join}\n{join}:\n"));
            } else {
                for (pi, (from_it, to_it)) in pairs.iter().enumerate() {
                    let matched = format!("{dest}_m{pi}");
                    let next = if pi + 1 < pairs.len() {
                        format!("{dest}_n{pi}")
                    } else {
                        join.clone()
                    };
                    s.push_str(&format!(
                        "  %{dest}_c{pi} = icmp eq ptr %{dest}_src_it, {from_it}\n\
                         \x20 br i1 %{dest}_c{pi}, label %{matched}, label %{next}\n\
                         {matched}:\n\
                         \x20 store ptr {to_it}, ptr %{dest}_va\n\
                         \x20 br label %{join}\n"
                    ));
                    if pi + 1 < pairs.len() {
                        s.push_str(&format!("{next}:\n"));
                    }
                }
                s.push_str(&format!("{join}:\n"));
            }
            return Some(s);
        }
        // class → iface
        if !self.layouts.classes.contains_key(view_ty) {
            return None;
        }
        let provider = self
            .itable_provider_for(view_ty, iface_ty)
            .unwrap_or_else(|| view_ty.to_string());
        let itable_iface = self
            .layouts
            .classes
            .get(provider.as_str())
            .and_then(|pc| {
                pc.interfaces
                    .iter()
                    .find(|n| {
                        let n = n.as_str();
                        n == iface_ty
                            || (iface_generic_root(n) == iface_generic_root(iface_ty)
                                && n.contains('_')
                                && iface_ty.contains('_'))
                    })
                    .map(|n| n.to_string())
            })
            .unwrap_or_else(|| iface_ty.to_string());
        Some(format!(
            "  %{dest} = alloca {{ ptr, ptr }}\n\
             \x20 %{dest}_oa = getelementptr inbounds {{ ptr, ptr }}, ptr %{dest}, i32 0, i32 0\n\
             \x20 store ptr %arg{i}, ptr %{dest}_oa\n\
             \x20 %{dest}_va = getelementptr inbounds {{ ptr, ptr }}, ptr %{dest}, i32 0, i32 1\n\
             \x20 store ptr @.itable.{provider}_{itable_iface}, ptr %{dest}_va\n"
        ))
    }

    /// `(sig_params, call_args)` for thunk：`ptr %self` + 按 **view** layout 声明的形参。
    fn thunk_param_forwarding(&self, param_types: &[Ident]) -> (String, String) {
        let mut sig = vec!["ptr %self".to_string()];
        let mut call = vec!["ptr %self".to_string()];
        for (i, pty) in param_types.iter().enumerate() {
            let llvm = llvm_field_type(pty.as_str(), self.layouts);
            sig.push(format!("{llvm} %arg{i}"));
            call.push(format!("{llvm} %arg{i}"));
        }
        (sig.join(", "), call.join(", "))
    }

    /// CD-10/D1/CD-11/D2：沿类解析接口方法/虚方法在本类的最终实现。
    ///
    /// 精确签名键（名+形参）命中 `method_impl` → 最派生实现类 → 其
    /// `declared_methods` 中同签名条目的 `link_name`（含重载消歧后缀）。
    /// 精确命中失败时（variance 适配视图参数不同 / 泛型擦除）按名沿链
    /// 回退到声明类（对齐旧语义，适配器 thunk 处理 ABI 差异）。
    fn resolve_method_impl(
        &self,
        class: &ClassLayout,
        method: &str,
        params: &[Ident],
    ) -> Option<(Ident, String)> {
        if let Some(impl_class) = class.method_impl.get(&(method.into(), params.to_vec())) {
            if let Some(entry) = self.layouts.classes.get(impl_class.as_str()).and_then(|c| {
                c.declared_methods
                    .iter()
                    .find(|m| m.name.as_str() == method && m.param_types == params)
            }) {
                return Some((impl_class.clone(), mangle_fn_name(&entry.link_name)));
            }
        }
        // 名称回退：沿类链找首个声明该名称的类（variance 视图参数不同等）。
        let mut cur = class.name.as_str();
        loop {
            let cl = self.layouts.classes.get(cur)?;
            if let Some(entry) = cl
                .declared_methods
                .iter()
                .find(|m| m.name.as_str() == method)
            {
                return Some((cur.into(), mangle_fn_name(&entry.link_name)));
            }
            let Some(p) = &cl.parent else {
                return None;
            };
            cur = p.as_str();
        }
    }

    fn class_method_return_type(
        &self,
        class: &str,
        method: &str,
        params: &[Ident],
    ) -> Option<String> {
        let mut cur = class;
        loop {
            let cl = self.layouts.classes.get(cur)?;
            if let Some(m) = cl
                .declared_methods
                .iter()
                .find(|m| m.name.as_str() == method && m.param_types == params)
            {
                return Some(m.return_type.to_string());
            }
            if let Some(m) = cl
                .declared_methods
                .iter()
                .find(|m| m.name.as_str() == method)
            {
                return Some(m.return_type.to_string());
            }
            cur = cl.parent.as_deref()?;
        }
    }

    fn class_method_param_types(&self, class: &str, method: &str, params: &[Ident]) -> Vec<Ident> {
        let mut cur = class;
        loop {
            let Some(cl) = self.layouts.classes.get(cur) else {
                return Vec::new();
            };
            if let Some(m) = cl
                .declared_methods
                .iter()
                .find(|m| m.name.as_str() == method && m.param_types == params)
            {
                return m.param_types.clone();
            }
            if let Some(m) = cl
                .declared_methods
                .iter()
                .find(|m| m.name.as_str() == method)
            {
                return m.param_types.clone();
            }
            match &cl.parent {
                Some(p) => cur = p.as_str(),
                None => return Vec::new(),
            }
        }
    }

    fn itable_provider_for(&self, class: &str, iface: &str) -> Option<String> {
        let mut cur = class;
        loop {
            let cl = self.layouts.classes.get(cur)?;
            if cl.interfaces.iter().any(|i| {
                let n = i.as_str();
                n == iface
                    || (iface_generic_root(n) == iface_generic_root(iface)
                        && n.contains('_')
                        && iface.contains('_'))
            }) {
                return Some(cur.to_string());
            }
            cur = cl.parent.as_deref()?;
        }
    }

    /// Emit a synthetic getter function for an interface property backed by a class field.
    fn emit_iface_property_getter(&self, class: &str, prop: &str, getter_fn: &str) -> String {
        let (offset, field_ty_str) = self
            .layouts
            .classes
            .get(class)
            .and_then(|c| c.fields.iter().find(|f| f.name.as_str() == prop))
            .map(|f| (f.offset, f.ty.to_string()))
            .unwrap_or((16, "ptr".into()));
        let llvm_ty = types::llvm_field_type(&field_ty_str, self.layouts);
        format!(
            "define {llvm_ty} @{getter_fn}(ptr %self) {{\n\
             entry:\n\
             \x20 %addr = getelementptr inbounds i8, ptr %self, i32 {offset}\n\
             \x20 %val = load {llvm_ty}, ptr %addr\n\
             \x20 ret {llvm_ty} %val\n\
             }}\n\n"
        )
    }

    /// RFC 004 M2：为用户类型键 K 发射 `@__dict_hash_{K}` 与 `@__dict_eq_{K}` trampoline。
    ///
    /// trampoline 适配 runtime `rt_hash_fn`/`rt_eq_fn` ABI（`void*` 参数、
    /// `uint32_t`/`int32_t` 返回）到用户类型的 `K_GetHashCode(K)` / `K_Equals(K, K)`
    /// 静态方法签名。用户类型以 `ptr` 传递（struct by-ref / class ptr），无需装箱。
    ///
    /// - `@__dict_hash_{K}(ptr %key) -> i32`：直接转发到 `@{K}_GetHashCode(ptr %key)`
    /// - `@__dict_eq_{K}(ptr %a, ptr %b) -> i32`：转发到 `@{K}_Equals(ptr %a, ptr %b)`，
    ///   将 `i1` 结果 `zext` 为 `i32`（runtime 约定：非零 = 相等）
    ///
    /// 若用户类型未定义 `GetHashCode`/`Equals` 静态方法，链接器报 undefined symbol。
    fn emit_dict_user_trampolines(&self, k: &str) -> String {
        let hash_name = dict_user_hash_fn(k);
        let eq_name = dict_user_eq_fn(k);
        let user_hash = format!("@{k}_GetHashCode");
        let user_eq = format!("@{k}_Equals");
        format!(
            "define i32 {hash_name}(ptr %key) {{\n\
             entry:\n\
             \x20 %h = call i32 {user_hash}(ptr %key)\n\
             \x20 ret i32 %h\n\
             }}\n\n\
             define i32 {eq_name}(ptr %a, ptr %b) {{\n\
             entry:\n\
             \x20 %r = call i1 {user_eq}(ptr %a, ptr %b)\n\
             \x20 %z = zext i1 %r to i32\n\
             \x20 ret i32 %z\n\
             }}\n\n"
        )
    }
}

/// Compute byte offset of each line start (RFC 024 M1: span → line/col).
/// Returns offsets including the implicit final line (after trailing newline).
fn compute_line_starts(source: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push((i + 1) as u32);
        }
    }
    starts
}

/// RFC 018 M2 step 6: abstract method stub 的简化返回类型映射。
///
/// 仅用于 stub 发射（返回 `zeroinitializer`），不参与正常 codegen 路径。
/// - 基元类型映射到对应 LLVM 类型
/// - class/struct/string/enum 等引用类型统一为 `ptr`
/// - 未知类型也统一为 `ptr`（防御性兜底）
fn simple_llvm_ret_ty(name: &str) -> &'static str {
    match name {
        "void" => "void",
        "bool" => "i1",
        "char" => "i32",
        "byte" => "i8",
        "short" => "i16",
        "int" | "TypeKind" => "i32",
        "long" => "i64",
        "float" => "float",
        "double" => "double",
        _ => "ptr", // class/struct/string/enum/List_*/Nullable 等引用类型
    }
}

/// RFC 038 M2 链接模型：RtTypeInfo 的 LLVM 布局串（25 字段，对齐 rt_abi.h
/// `RtTypeInfo`；与 `emit_typeinfos` / `emit_runtime_decls` 的
/// `@rt_typeinfo_<prim>` 声明同串）。供外部类 typeinfo 的
/// `external global` 声明使用——链接只看符号名，类型串仅约束本 TU 内
/// 访问方式（外部声明只取地址，不 load 内容）。
const RT_TYPEINFO_LLVM_TY: &str = "{ i32, ptr, ptr, ptr, ptr, i32, i32, \
                     ptr, i32, ptr, i32, ptr, i32, ptr, i32, ptr, i32, \
                     ptr, i32, ptr, ptr, i32, ptr, i32, ptr }";

/// RFC 017 阶段一：基元类型名 → `rt_typeinfo_prim(id)` 查询 id（id 序与
/// rt_type.c `rt_primitive_table` 一致）；非基元返回 `None`。供
/// `emit_typeinfos` pending 登记与 `try_emit_typeof_as_runtime_type` /
/// `emit_box` 共享，保持单一判定来源。
fn primitive_typeinfo_id(type_name: &str) -> Option<i32> {
    Some(match type_name {
        "int" => 0,
        "long" => 1,
        "short" => 2,
        "byte" => 3,
        "char" => 4,
        "float" => 5,
        "double" => 6,
        "bool" => 7,
        "string" => 8,
        "void" => 9,
        "object" => 10,
        _ => return None,
    })
}

/// RFC 038 M2 链接模型：typeinfo 符号判定核心（只读、无借用副作用）——
/// 命名类型（class/struct/interface/enum）映射 `@.typeinfo.{T}`；数组
/// （`_arr`）与不在 layouts 的复合类型无 typeinfo 全局，返回 `None`
/// （调用方发 `null`）。供 `typeinfo_global_for`（ModuleEmitter）与
/// `FnEmitter::typeinfo_global`（经同一布局判定）及 `emit_typeinfos`
/// 循环内 pending 登记路径共享，保持单一判定来源。
///
/// RFC 017 阶段一：基元 typeinfo 数据符号已在 runtime 侧 static 化，不可
/// 跨共享库映像引用——基元一律返回 `None`（反射数组槽初值 null + pending
/// 回填；指令语境经 `primitive_typeinfo_id` → `rt_typeinfo_prim(id)` call
/// 查询），调用方不得再发射 `@rt_typeinfo_{prim}` 引用。
fn typeinfo_symbol_core(layouts: &ProgramLayouts, type_name: &str) -> Option<String> {
    // 已注册 nominal（class/struct/interface/enum）优先于 `_arr` 后缀启发式：
    // 泛型实例 mangle 同样以 `_arr` 结尾（`List<string[]>` → `List_string_arr`），
    // 但它是真实类、有 `@.typeinfo.{T}` 全局；数组 mangle 名不是 nominal 类型、
    // 不进 layouts，可判别。
    if layouts.classes.contains_key(type_name)
        || layouts.structs.contains_key(type_name)
        || layouts.interfaces.contains_key(type_name)
        || layouts.enums.contains(type_name)
    {
        return Some(format!("@.typeinfo.{type_name}"));
    }
    // 数组（`{Elem}_arr`）、函数指针 / 委托及其它 ctype 类型**没有** typeinfo
    // 全局常量——不在 layouts 的名字（含数组 mangle）一律返回 None。
    None
}

/// RFC 017 M4-link Phase B：发射模块级 `$<name> = comdat any` 声明段。
///
/// Windows COFF 上 `linkonce_odr` linkage 必须配合显式 `comdat` 指令才能跨
/// `.o` 去重——LLVM 在 COFF 目标上不会自动为 `linkonce_odr` 函数创建
/// COMDAT 段，导致 lld-link 报 `duplicate symbol`。本函数为每个名字生成
/// `$<name> = comdat any` 模块级声明，配合 `define` 行的 `comdat` 属性
/// （由 `FnEmitter::comdat_attr` / 第 6 步默认 ctor 附加）形成完整 COMDAT group。
///
/// `comdat any` 选择策略：链接器从所有同名 COMDAT group 中任选一份，丢弃
/// 其余。这契合 `linkonce_odr` 的 ODR 语义（所有副本语义等价，取一即可）。
///
/// 空列表返回空串（无 comdat 段输出，对模块格式无影响）。
fn emit_comdat_decls(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(
        "; ---- RFC 017 M4-link Phase B: COMDAT declarations for linkonce_odr symbols ----\n",
    );
    for name in names {
        out.push_str(&format!("${name} = comdat any\n"));
    }
    out.push('\n');
    out
}

/// Resolve a Span to (line, col) — 1-based (RFC 031 §2).
/// Returns (0, 0) for DUMMY spans (file_id == 0).
fn span_to_line_col(span: Span, line_starts: &[u32]) -> (i32, i32) {
    if span.file_id == 0 || line_starts.is_empty() {
        return (0, 0);
    }
    let offset = span.start;
    let line_idx = match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    let line_start = line_starts[line_idx];
    let col = offset - line_start + 1;
    ((line_idx as i32) + 1, col as i32)
}

/// 单个追加循环的游标提升状态（RFC 005）。
/// `handle` 为提升后的 rt_sb_t 头指针（preheader 一次 load，LLVM LICM 视
/// 其为循环不变量）；`data/len/cap` 为 shadow alloca（SROA 转寄存器）。
#[derive(Clone)]
struct SbShadow {
    /// rt_sb_t 头指针（SSA 名称）。
    handle: String,
    /// shadow alloca：字符缓冲指针（ptr）。
    data: String,
    /// shadow alloca：当前长度（i64）。
    len: String,
    /// shadow alloca：容量（i64）。
    cap: String,
}

/// RFC 005 CFG 级追加循环提升计划：由 `loop_backedges` 识别的 flag 式
/// `while(flag){ if(cond){ sb.Append(char); … } else { flag=false } }` 循环。
/// 在 preheader 建 shadow alloca 并初始化，exit 一次性 flush 回堆头。
#[derive(Clone)]
struct SbPromoteLoop {
    /// 循环前置头块（唯一 `Goto(header)` 且非 backedge 的块）。
    preheader: mir::BlockId,
    /// 纯追加体块（嵌套 If 的 then 目标，含 `sb.Append(char)`）。
    body: mir::BlockId,
    /// 循环出口块（header `CondBr` 的 else 目标）。
    exit: mir::BlockId,
    /// StringBuilder 接收者 local（`%v{id}` 持有对象指针）。
    receiver: LocalId,
}

/// 新候选 `(preheader, exit)` 是否与已收录的任一候选嵌套（含互相包含）。
/// 单 `sb_shadow` 状态无法承载嵌套提升，嵌套候选一律拒绝。`to_cfg` 按 DFS
/// 序分配块 id，循环块落在 preheader 与 exit 之间，故用 id 序判定区间互斥。
fn sb_shadow_nested(
    existing: &[SbPromoteLoop],
    preheader: mir::BlockId,
    exit: mir::BlockId,
) -> bool {
    let (np, ne) = (preheader.0, exit.0);
    existing.iter().any(|c| {
        let (cp, ce) = (c.preheader.0, c.exit.0);
        // 新候选包含已有候选，或已有候选包含新候选。
        (cp < np && ne < ce) || (np < cp && ce < ne)
    })
}

/// Function-level emitter: converts a single MirCfgBody to LLVM IR text.
struct FnEmitter<'a> {
    cfg: mir::MirCfgBody,
    layouts: &'a ProgramLayouts,
    external_class_names: &'a HashSet<String>,
    /// RFC 038 M2：函数体引用、且不在本 TU 发射的外部类聚合全局登记表
    /// （`@.vtable.{Ext}`）。与 ModuleEmitter.external_aggregate_refs 共享同一
    /// BTreeMap——FnEmitter 在 `vtable_global` 中按需登记，emit_module 末尾统一
    /// 发射 `@<sym> = external global <ty>` 声明，使 `store ptr @.vtable.{Ext}`
    /// 有定义可解析（消费者 MainObject 的 linkonce_odr COMDAT 提供）。
    external_aggregate_refs: &'a mut std::collections::BTreeMap<String, String>,
    output: String,
    temp_counter: u32,
    label_counter: u32,
    string_seen: &'a HashMap<String, String>,
    /// RFC 017 M2：codegen 期动态字符串常量（Entry 符号名 / 异常消息）。
    string_consts: &'a mut StringConstAccumulator,
    async_fns: &'a HashSet<String>,
    is_main: bool,
    is_windows: bool,
    /// RFC 008: locals that hold an `arc_closure` value (set by
    /// `let f = <capturing-lambda>`). `emit_indirect_call` checks this to
    /// decide whether to extract fn_ptr/env_ptr from the closure or treat
    /// the local as a bare function pointer.
    closure_locals: HashSet<LocalId>,
    /// 刀 2.2 跨块 ARC 优化：dead-copy（arc-neutral）局部集合。此类 class 局部
    /// 从未被读取、仅以拷贝语义赋值——codegen 跳过其赋值的 `rt_arc_inc`/dec 与
    /// epilogue dec（整对跨块消除，见 `mir::find_dead_arc_locals`）。仅同步函数
    /// 计算；async/SM 为空集。
    dead_arc_locals: HashSet<LocalId>,
    /// 闭包逃逸安全：被嵌套闭包 **ByRef** 捕获的宿主局部（见
    /// `mir::find_byref_captured_locals`）。这些局部按 C# display-class 语义
    /// 提升为堆槽（`malloc` 槽替代 `alloca`），闭包与宿主函数共享该堆槽，
    /// 宿主帧返回后闭包仍能读正确值（否则读死栈槽 → 垃圾值/崩溃）。
    /// 仅同步函数/M1 fallback 参与 alloca 替换；async SM 局部本就在 env 堆槽。
    byref_captured_locals: HashSet<LocalId>,
    /// RFC 024 M1: line start offsets for span → line/col resolution.
    line_starts: &'a [u32],
    /// RFC 031 §2: DWARF 5 debug metadata emitter (borrowed from ModuleEmitter).
    dbg: &'a mut debug_info::DbgMetadata,
    /// RFC 031 §2: DISubprogram metadata ID for the current function.
    /// 0 = debug info disabled (no `!dbg` attached).
    subprogram_id: u32,
    /// A1 (debt repayment): finally 块栈，用于 return/throw 时正确执行 finally 语义。
    /// `emit_try_finally` 进入 body 前 push，body 完成后 pop。
    /// Return/Throw 时 clone 栈快照并 inline 执行所有 finally 块，确保资源释放。
    finally_stack: Vec<Vec<mir::MirStatement>>,
    /// Region 体内嵌套 while 的 `(exit_label, continue_label)` 栈；
    /// `to_cfg` 未展平的 Break/Continue 由此解析最近循环。
    nested_loop_stack: Vec<(String, String)>,
    /// RFC 005 追加循环游标提升：当前纯 `sb.Append(char)` 循环的 shadow 状态。
    /// 命中时，StringBuilder 头字段（data/len/cap）被提升为 shadow alloca
    /// （SROA → 寄存器），热循环不再触碰堆头；冷路径（容量不足）先 flush
    /// 回头、调用后 re-sync reload。None = 不在提升循环内。
    sb_shadow: Option<SbShadow>,
    /// RFC 005 shadow 提升 `handle/data/len/cap` 按纯追加体块索引。`to_cfg`
    /// 按 DFS 序分配块 id，循环出口块往往先于体块发射——跨块瞬态 `sb_shadow`
    /// 会在退出口时被提前 `take()`。此表在 preheader 发射时填充、体块发射时
    /// 取用，使发射顺序与 CFG 顺序解耦。
    sb_shadow_map: std::collections::HashMap<mir::BlockId, SbShadow>,
    /// RFC 005 CFG 级提升计划：当前函数内所有可提升的纯追加循环
    /// （`find_sb_promote_loops` 在 CFG 发射前计算一次）。
    sb_promotes: Vec<SbPromoteLoop>,
    /// Region 体内 break/continue/return 后置 true，跳过死代码与多余 br。
    flow_terminated: bool,
    /// Alloca instructions that must be emitted in the entry block (before any
    /// basic block label). Holds expression-temporary allocas hoisted out of
    /// loop bodies (e.g. `list_item_to_ptr` item slots) which must be static
    /// stack allocations, not dynamic ones in a non-entry block. Flushed via
    /// `flush_entry_allocas` before each function's entry terminator.
    entry_allocas: String,
    /// P1-B2：正在发射「带 catch 的 try body」时为 true。
    /// 此区域内的 Throw 由同层 catchswitch 接住，跳过 `emit_finally_chain`。
    /// 进入 catch 体时置 false，使 `when` rethrow 仍执行外层 finally。
    emitting_caught_try_body: bool,
    /// Zero-cost EH milestone ② (Windows SEH)：当前 try 区域的 `catchswitch`
    /// 块标签栈。try region 内 may-throw 调用点发 `invoke … unwind label %cs`，
    /// 使异常落入本区域 catchswitch。栈为空 = 不在任何 try region 内（POSIX
    /// Itanium 属里程碑⑨，此栈不参与）。
    eh_region_stack: Vec<String>,
    /// Zero-cost EH milestone ③ (Windows SEH)：**外层** finally cleanup 分发
    /// 块标签栈。内层 `catchswitch`/`cleanupret` 的 unwind 目标为最近外层
    /// finally 的 cleanup 分发块（未匹配/继续 unwind 时先跑外层 finally），
    /// 无外层 finally 时 `unwind to caller`。栈空 = 不在任何 finally region 内。
    eh_cleanup_stack: Vec<String>,
    /// Native contract symbol table (RFC 016 M1).
    native_symbols: &'a native::NativeSymbolTable,
    /// RFC 016：运行时加载 native 模块信息（懒解析器符号 + 函数表槽位）。
    /// 空表 = 无 runtime 模块（`try_emit_native_call` 直接走静态直连路径）。
    runtime_native: &'a native::RuntimeModuleInfos,
    /// RFC 016 M1：native callback 类型表。
    native_callback_table: &'a emit_native_callback::NativeCallbackTable,
    /// RFC 016 M1：模块级 trampoline 累积器（由 FnEmitter 在
    /// `try_emit_native_call` 中按需推入，emit_module 末尾统一发射）。
    /// 每个 trampoline 适配一个 Arc 函数指针到 C ABI callback 类型。
    native_trampolines: &'a mut emit_native_callback::NativeTrampolineAccumulator,
    /// RFC 025 M5：字典枚举幻影类发射累积器（见 ModuleEmitter 同名字段）。
    dict_enum_artifacts: &'a mut std::collections::HashSet<String>,
    /// RFC 016 M2: 有捕获 lambda 传给 native callback 后待清理的 TLS slot 列表。
    /// native call 返回后逐个 `rt_ffi_clear_callback`。
    pending_ffi_slots: Vec<i32>,
    /// RFC 009 M2: 当前是否在状态机 resume 函数内发射代码。
    /// 为 true 时，`emit_terminator` 的 Return 分支走状态机 return 逻辑
    ///（设置 state=-1 + 写 result 到 Task 句柄 + ret i32 0）。
    in_state_machine: bool,
    /// RFC 009 M2: 状态机 env struct 类型名（如 `%struct.__async_env_Main`）。
    /// 在 `emit_async_state_machine` 中设置，供 `emit_terminator` Return 分支使用。
    sm_env_type: String,
    /// RFC 009 M2 整图 CFG：`(block_id, stmt_path) → await 全局序号`。
    /// stmt_path 为块顶层语句下标到嵌套 region（try/catch/finally/if/while/linq）
    /// body 的完整下标路径（RFC 004 里程碑⑦：await 可位于 try 区域内）。
    sm_await_index: HashMap<(u32, Vec<usize>), usize>,
    /// RFC 009 M2 整图 CFG：本函数 await 总数（状态编号用）。
    sm_await_count: usize,
    /// RFC 016：跨 await 存活的局部集合（MIR 侧 liveness pass 输出）。
    /// 仅这些局部提升为 env 字段并参与 env 唯一 owner 的 ARC 配对面；
    /// 未存活局部零 env 字段、零 save/load、零 dtor 配对。
    await_live_locals: HashSet<LocalId>,
    /// RFC 016：env 局部 → env 字段索引（3 + 在 env_local_ids 中的序号）。
    /// 替代旧的固定 `3 + local_id.0` 布局，实现「只提升存活局部」。
    sm_env_local_index: HashMap<LocalId, usize>,
    /// RFC 016：resume 函数级 EH cleanup pad 标签。resume body 发射期间为
    /// `Some`；任何 unwind（faulted throw / 异常传播）经 invoke 落入该 pad，
    /// cleanup 一次性调用 dtor 释放 env 持有的 class 引用 + 释放 env。
    sm_cleanup_label: Option<String>,
    /// RFC 009 I1：当前是否在协程（pre-split `llvm.coro.*`）路径内发射。
    /// 为 true 时，await / return / drop 走协程分派（emit_async_coro）。
    in_coroutine: bool,
    /// RFC 009 I1：协程所属 Task* 的帧槽 alloca 名（跨 suspend 存活，
    /// return 写结果 / await 登记 waker 时读取）。
    coro_task_slot: String,
    /// RFC 009 I1：协程尾部共享标签——final suspend（return 汇聚点）。
    coro_final_label: String,
    /// RFC 009 I1：协程尾部共享标签——cleanup（destroy 入口，dec 帧持有
    /// 引用 + coro.free）；各 suspend 点 switch 的 `i8 1` 分支目标。
    coro_cleanup_label: String,
    /// RFC 009 I1：协程尾部共享标签——yield 返回（coro.end + ret task）；
    /// 各 suspend 点 switch 的 default 分支目标。
    coro_ret_label: String,
    /// RFC 009 I1：协程路径 await 发射计数器（与 preamble 预发的
    /// `%__coro_awaiter_N` 帧槽双射）。
    coro_await_counter: usize,
    /// 当前正在发射的语句在块内下标（供 await 位点索引）。
    current_stmt_index: usize,
    /// 当前语句嵌套路径：块顶层语句下标 + 各嵌套 region body 下标
    /// （与 `sm_await_index` 的 key 第二分量一致；顶层为 `[stmt_idx]`）。
    stmt_path: Vec<usize>,
    /// RFC 023 M1: DI 工厂函数累积器（ModuleEmitter 共享借用）。
    /// FnEmitter 通过 `ensure_factory_generated` 推入工厂 IR；ModuleEmitter 在
    /// emit_module 末尾统一发射。按 TImpl 去重（DiFactoryAccumulator.names）。
    di_factories: &'a mut emit_di::DiFactoryAccumulator,
    /// RFC 004 M1: 用户函数返回类型表（key=函数名，value=cfg.ret）。
    /// 由 `emit_module` 预收集所有 `fns` 的返回类型构建，供 `emit_call_typed`
    /// 在 user function call 路径查询真实返回类型，避免依赖 `expected`
    /// （`emit_rvalue` 不带 typed 入口传默认 `Int`，对返回 bool/string/long 等
    /// 的函数会生成 `call i32 @Fn(...)` 与实际 `define i1 @Fn(...)` 类型错配）。
    fn_returns: &'a HashMap<String, TypeId>,
    /// RFC 015 Phase B.7：模块 call-graph `nounwind` 表（name → nounwind）。
    nounwind_map: &'a HashMap<String, bool>,
    /// RFC 009 M3：按需 spill 集合——env struct 中转为 ptr 的 large local。
    /// 空集表示无 spill（M2 行为，全 hoist 为值类型）。
    /// Key = local index (usize, 对应 LocalId.0)。
    spill_set: HashSet<usize>,
    /// RFC 009 M3：当前正在发射的 CFG 块 ID。`emit_cfg_block` 入口设置，
    /// 供 `emit_terminator` 判定 Goto 是否为 while 循环 backedge
    ///（记录于 `cfg.loop_backedges`），决定是否附加 `!llvm.loop` metadata。
    current_block_id: mir::BlockId,
    /// RFC 009 M6：ForEach trampoline 计数器，为每个 ForEach 调用点生成
    /// 唯一的 trampoline 函数名 `__foreach_tramp_N`。
    foreach_tramp_counter: u32,
    /// RFC 009 M5.7：Parallel.For trampoline 计数器，为每个 Parallel.For 调用点
    /// 生成唯一的 trampoline 函数名 `__parallel_for_tramp_N`（衔接 runtime
    /// `body(i, env)` ABI 与 Arc 闭包 `fn(env, idx_ptr)` ABI）。
    parallel_for_tramp_counter: u32,
    /// RFC 039 M2：栈局部 alloca 的 lifetime 区间表 `(slot_ptr, byte_size)`。
    /// 在 entry 块发射 `!llvm.lifetime.start` 时填充；同步 return 路径（epilogue）
    /// 据此发射配套的 `!llvm.lifetime.end`，供 StackColoring / 栈槽复用把
    /// 已死局部提前释放。仅含尺寸精确可知（标量 / ptr 槽）的局部——struct /
    /// vector 等未知尺寸槽跳过，规避低估尺寸导致的误编译。
    stack_lifetime: Vec<(String, u64)>,
}

impl<'a> FnEmitter<'a> {
    fn new(
        body: &'a MirCfgBody,
        layouts: &'a ProgramLayouts,
        external_class_names: &'a HashSet<String>,
        external_aggregate_refs: &'a mut std::collections::BTreeMap<String, String>,
        string_seen: &'a HashMap<String, String>,
        string_consts: &'a mut StringConstAccumulator,
        async_fns: &'a HashSet<String>,
        is_windows: bool,
        line_starts: &'a [u32],
        dbg: &'a mut debug_info::DbgMetadata,
        subprogram_id: u32,
        native_symbols: &'a native::NativeSymbolTable,
        runtime_native: &'a native::RuntimeModuleInfos,
        native_callback_table: &'a emit_native_callback::NativeCallbackTable,
        native_trampolines: &'a mut emit_native_callback::NativeTrampolineAccumulator,
        dict_enum_artifacts: &'a mut std::collections::HashSet<String>,
        di_factories: &'a mut emit_di::DiFactoryAccumulator,
        fn_returns: &'a HashMap<String, TypeId>,
        nounwind_map: &'a HashMap<String, bool>,
    ) -> Self {
        Self {
            cfg: body.clone(),
            layouts,
            external_class_names,
            external_aggregate_refs,
            output: String::new(),
            temp_counter: 0,
            label_counter: 0,
            string_seen,
            string_consts,
            async_fns,
            is_main: false,
            is_windows,
            closure_locals: HashSet::new(),
            dead_arc_locals: mir::find_dead_arc_locals(body, layouts),
            byref_captured_locals: mir::find_byref_captured_locals(body),
            line_starts,
            dbg,
            subprogram_id,
            finally_stack: Vec::new(),
            nested_loop_stack: Vec::new(),
            sb_shadow: None,
            sb_shadow_map: std::collections::HashMap::new(),
            sb_promotes: Vec::new(),
            flow_terminated: false,
            emitting_caught_try_body: false,
            eh_region_stack: Vec::new(),
            eh_cleanup_stack: Vec::new(),
            native_symbols,
            runtime_native,
            native_callback_table,
            native_trampolines,
            dict_enum_artifacts,
            pending_ffi_slots: Vec::new(),
            in_state_machine: false,
            sm_env_type: String::new(),
            sm_await_index: HashMap::new(),
            sm_await_count: 0,
            await_live_locals: HashSet::new(),
            sm_env_local_index: HashMap::new(),
            sm_cleanup_label: None,
            in_coroutine: false,
            coro_task_slot: String::new(),
            coro_final_label: String::new(),
            coro_cleanup_label: String::new(),
            coro_ret_label: String::new(),
            coro_await_counter: 0,
            current_stmt_index: 0,
            stmt_path: Vec::new(),
            di_factories,
            fn_returns,
            nounwind_map,
            entry_allocas: String::new(),
            spill_set: body.spill_set.spilled.clone(),
            // RFC 009 M3：初始为 entry 块，emit_cfg_block 入口会更新。
            current_block_id: mir::BlockId(0),
            // RFC 009 M6：ForEach trampoline 计数器从 0 开始。
            foreach_tramp_counter: 0,
            // RFC 009 M5.7：Parallel.For trampoline 计数器从 0 开始。
            parallel_for_tramp_counter: 0,
            // RFC 009 M2：lifetime 区间表初始为空，emit_sync_function 填充。
            stack_lifetime: Vec::new(),
        }
    }

    /// RFC 017 M4-link Phase B：返回 LLVM IR `define` 行的 linkage 前缀。
    ///
    /// - `External` → `""`（默认 external linkage，单一定义来源）
    /// - `LinkonceOdr` → `"linkonce_odr "`（跨 `.o` 弱符号去重，ODR 保证语义等价）
    /// - `DeclareOnly` → 不应出现于 `MirCfgBody`（外部声明由 `declare_emitter`
    ///   直接消费 `typeck::external_symbols` 列表发射 `declare`）；为防御性
    ///   回退为 `""`（external）。
    fn linkage_prefix(&self) -> &'static str {
        match self.cfg.linkage {
            mir::Linkage::External => "",
            mir::Linkage::LinkonceOdr => "linkonce_odr ",
            mir::Linkage::DeclareOnly => "",
        }
    }

    /// RFC 017 M4-link Phase B：返回 LLVM IR `define` 行的 `comdat` 属性片段。
    ///
    /// **仅当 linkage 为 `LinkonceOdr` 时返回 `" comdat"`**——Windows COFF 上
    /// `linkonce_odr` 若无显式 `comdat` 指令，lld-link 仍报 `duplicate symbol`：
    /// LLVM IR 在 COFF 目标上不会自动为 `linkonce_odr` 函数创建 COMDAT 段，
    /// 必须显式声明 `$<name> = comdat any` 并在 `define` 行附加 `comdat` 属性，
    /// 链接器才能跨 `.o` 按名去重。Linux ELF 上 `comdat` 指令被映射为 section
    /// group，行为与隐式 `linkonce_odr` 等价，因此本方法对 ELF 无副作用。
    ///
    /// 配套的模块级 `$<name> = comdat any` 声明由 `emit_module` 第 4d 步统一发射，
    /// 见 `emit_comdat_decls`。`External` linkage 不返回 `comdat`——单一权威定义
    /// 来源不需要去重。
    fn comdat_attr(&self) -> &'static str {
        match self.cfg.linkage {
            mir::Linkage::LinkonceOdr => " comdat",
            mir::Linkage::External | mir::Linkage::DeclareOnly => "",
        }
    }

    /// Whether DWARF 5 debug info is enabled for this function (RFC 017 M2).
    fn dbg_enabled(&self) -> bool {
        self.dbg.enabled() && self.subprogram_id != 0
    }

    /// Render the `!dbg !N` attribute for function definitions (RFC 031 §2).
    /// Returns empty string when debug info is disabled.
    fn dbg_attr(&self) -> String {
        if self.dbg_enabled() {
            format!(" !dbg !{}", self.subprogram_id)
        } else {
            String::new()
        }
    }

    /// Emit a DILocation for the given span and return the metadata node ID.
    /// Returns 0 if debug info is disabled or the span is DUMMY.
    fn emit_dilocation(&mut self, span: Span) -> u32 {
        if !self.dbg_enabled() {
            return 0;
        }
        let (line, col) = self.span_to_line_col(span);
        self.dbg
            .add_location(line as u32, col as u32, self.subprogram_id)
    }

    /// Resolve a Span to (line, col) — 1-based (RFC 024 M1).
    /// Returns (0, 0) for DUMMY spans (file_id == 0).
    fn span_to_line_col(&self, span: Span) -> (i32, i32) {
        span_to_line_col(span, self.line_starts)
    }

    fn fresh_temp(&mut self) -> String {
        let n = self.temp_counter;
        self.temp_counter += 1;
        format!("%t{n}")
    }

    fn fresh_label(&mut self) -> String {
        let n = self.label_counter;
        self.label_counter += 1;
        format!("nest{n}")
    }

    fn local_ptr(&self, id: LocalId) -> String {
        format!("%v{}", id.0)
    }

    /// out/ref 实参的 byref 目标地址（转发语义对齐 C#/CLI byref：被调方写入
    /// 调用方原始存储，调用后调用方读取到写入值）。
    ///
    /// - 普通值局部：返回槽地址（`%vN`），被调方直接写入该槽。
    /// - `Ref` 形参局部（转发 byref）：槽内存的是调用方变量的指针，须 `load`
    ///   该指针转发给被调方；否则被调方把值写进指针槽，调用方变量收不到值
    ///   （user→stub 转发 out 丢失缺陷，CD-7 邻域）。与 `emit_rvalue` 的
    ///   `MirOperand::AddrOf` 分支保持同构。
    fn byref_arg_ptr(&mut self, id: LocalId) -> String {
        if matches!(self.local_type(id), TypeId::Ref { .. }) {
            let slot_ptr = self.local_ptr(id);
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = load ptr, ptr {slot_ptr}"));
            tmp
        } else {
            self.local_ptr(id)
        }
    }

    fn local_type(&self, id: LocalId) -> TypeId {
        self.cfg
            .locals
            .get(&id)
            .map(|(_, t)| t.clone())
            .unwrap_or(TypeId::Int)
    }

    fn emit(&mut self, line: &str) {
        self.output.push_str("  ");
        self.output.push_str(line);
        self.output.push('\n');
    }

    /// LLVM WinEH requires every `call` inside a cleanup funclet to carry the
    /// `"funclet"("token" %cp)` operand; without it, WinEHPrepare silently
    /// drops the calls (observed: finally cleanup bodies emptied by clang -O0,
    /// Milestone ⑦). Rewrites the emitted funclet-body slice in place.
    fn annotate_funclet_calls(&mut self, start: usize, end: usize, cp: &str) {
        if end <= start {
            return;
        }
        let mut buf = String::with_capacity(end - start + 64);
        for line in self.output[start..end].split('\n') {
            let t = line.trim_start();
            let is_call = t.starts_with("call ") || t.contains(" = call ");
            if is_call && !t.contains("funclet") {
                buf.push_str(line);
                buf.push_str(&format!(" [ \"funclet\"(token {cp}) ]"));
            } else {
                buf.push_str(line);
            }
            buf.push('\n');
        }
        self.output.replace_range(start..end, &buf);
    }

    /// Flush hoisted entry-block allocas (`entry_allocas`) into the current
    /// function's entry block. Must run before the entry block's terminator
    /// (`br`/`switch`) in every function-emission path; clears the buffer so
    /// successive functions in the same FnEmitter never inherit stale allocas.
    fn flush_entry_allocas(&mut self) {
        if self.entry_allocas.is_empty() {
            return;
        }
        self.output.push_str(&self.entry_allocas);
        self.entry_allocas.clear();
    }

    /// Emit an instruction with a `!dbg !N` suffix (RFC 031 §2).
    /// `loc_id` is the DILocation metadata node ID (from `emit_dilocation`).
    /// When `loc_id` is 0 or debug info is disabled, falls back to plain `emit`.
    fn emit_dbg(&mut self, line: &str, loc_id: u32) {
        if loc_id != 0 && self.dbg_enabled() {
            self.output.push_str("  ");
            self.output.push_str(line);
            self.output.push_str(&format!(", !dbg !{}", loc_id));
            self.output.push('\n');
        } else {
            self.emit(line);
        }
    }

    fn intern_string(&mut self, s: &str) -> String {
        if let Some(g) = self.string_seen.get(s) {
            return g.clone();
        }
        // RFC 017 M2：MIR 未预收集的动态字符串（Entry 符号名 / 异常消息等
        // codegen 期构造）→ 累积为模块级私有全局，避免回退 @.str.0 静默错串。
        self.string_consts.intern(s)
    }
}
