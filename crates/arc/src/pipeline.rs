//! Compiler pipeline: parse → hir → typeck (Pass 2–4) → borrowck(HIR) → mir → codegen.
//!
//! ## Release tree-shaking (RFC 034 M2 + codegen integration)
//!
//! `filter_reachable_mir_fns` 执行 BFS 入口可达性分析，剔除不可达 MIR 函数，
//! 从源头杜绝无用代码进入 LLVM IR。仅当存在 main/Entry 入口点时启用。
//! 另强制保留：接口 itable 实现方法、Dictionary 用户键 `Equals`/`GetHashCode`
//!（trampoline 以函数指针引用，MIR 无 Call 边），并对其 callee 做传递闭包扩展
//!（否则如 `GetService` 内 `throw new Exception(...)` 会留下对已剪除
//! `__ctor::Exception_1` 的调用）。
//!
//! ## RFC 009 macro passes
//!
//! `prepare_compilation` 在 `check_module`（Pass 2）之后调用 `run_pass3` /
//! `run_pass4`；无宏时为 no-op。

use crate::equipment::{ArtifactEmitter, ArtifactRequest, EmitRole, Equipments};
use crate::loader::{load_compile_unit, load_compile_unit_from_dir, CompileUnit, FileRegistry};
use arcgr::{EdgeKind, EntryPoint, EntryPointKind, ReferenceEdge as Edge};
use ast::{Expr, Item, Span, Stmt, Type, TypeId};
use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use reachability::{AnalysisInput, VirtualDispatchGroup};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// RFC 032 Phase 2c P1: InlineData 参数值——支持 int / string / bool 三种字面量类型。
#[derive(Clone, Debug)]
enum QifInlineArg {
    Int(i64),
    String(String),
    Bool(bool),
}

impl std::fmt::Display for QifInlineArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QifInlineArg::Int(n) => write!(f, "{n}"),
            QifInlineArg::String(s) => write!(f, "\"{s}\""),
            QifInlineArg::Bool(b) => write!(f, "{b}"),
        }
    }
}

impl QifInlineArg {
    /// 用于显示名——字符串不加引号，避免嵌套引号问题。
    fn display_fmt(&self) -> String {
        match self {
            QifInlineArg::Int(n) => n.to_string(),
            QifInlineArg::String(s) => s.clone(),
            QifInlineArg::Bool(b) => b.to_string(),
        }
    }
}

/// RFC 034 Phase 2c: 单个 QIF 测试方法条目（供生成 __QifTestHost.Main 用）。
#[derive(Clone, Debug)]
pub struct QifTestMethod {
    class_name: String,
    method_name: String,
    /// "Fact" | "Theory"
    attr_name: String,
    /// InlineData 参数（Theory 使用），每行一组参数
    inline_data: Vec<Vec<QifInlineArg>>,
    /// [Order(N)] 值（默认 0）
    order: i32,
    /// [Fact]/[Theory].DisplayName（空则用 class.method）
    display_name: String,
    /// 构造函数参数类型名列表（用于 IClassFixture / DI 注入）
    ctor_param_types: Vec<String>,
    /// 方法是否为 async（返回 Task / Task\<T\>），对标 xUnit [Fact] async 支持。
    is_async: bool,
    /// [Collection("name")] 测试集合名（类级属性，同集合内串行）
    collection_name: Option<String>,
    /// [Fact(Skip = "reason")] / [Theory(Skip = "reason")]——跳过原因
    skip_reason: Option<String>,
    /// [Trait("name", "value")]——类级 + 方法级 trait 对
    traits: Vec<(String, String)>,
    /// 命名空间（用于 --namespace 前缀过滤）
    namespace: String,
}

/// RFC 038: 声明式 `[AITool]` 工具方法条目（供生成 `__AIToolHost`）。
#[derive(Clone, Debug)]
struct AIToolMethod {
    /// 工具类完整路径（命名空间.类名；全局命名空间仅类名）
    class_path: String,
    method_name: String,
    /// 工具名（`[AITool("name")]` 位置参数 0；缺省用方法名）
    tool_name: String,
    description: String,
    capability: String,
    require_approval: bool,
    /// 参数绑定集合（标量 / 模型；含 `[Description]` 驱动的 schema 与反序列化）。
    params: Vec<AIToolParam>,
    /// 返回类型："void"/"string"/"Task<string>"/"Task<void>"
    ret: String,
    /// 构造方式："noarg" / "provider"（DI 桥）
    ctor_kind: String,
}

/// 工具方法参数绑定：标量（扁平 JSON 字段）或模型（用户类，整段参 JSON 反序列化）。
///
/// `[AITool]` + 参数级/模型字段级 `[Description]` 共同构成 schema 与反序列化依据。
#[derive(Clone, Debug)]
struct AIToolParam {
    /// 参数名。
    name: String,
    /// 标量类型名（"string"/"int"/"long"/"double"/"bool"/"string[]"）或模型类完整路径。
    ty: String,
    /// 参数级 `[Description]`（可为空）。
    description: String,
    /// 模型参数时非空：模型公开字段/属性（含 `[Description]`），生成嵌套 schema + 绑定。
    model_fields: Vec<AIToolModelField>,
}

/// 模型参数的一个可绑定字段（公开字段或公开 set 属性）。
#[derive(Clone, Debug)]
struct AIToolModelField {
    name: String,
    ty: String,
    description: String,
}

/// QIF 编译选项——从 arc.toml [qif] 节 + CLI 参数合并。
#[derive(Clone, Debug, Default)]
pub struct QifCompileOptions {
    /// 报告格式：human | json | junit
    pub output_format: String,
    /// 测试过滤表达式（QIF-6：XUnit 风格 `Field~Value` + `|`/`&`/`!` 组合）
    pub filter: String,
    /// 按命名空间前缀选择（与 filter 叠加为 AND；QIF-9）
    pub namespace: String,
    /// 按测试 Kind 选择（Fact/Theory/...；与 filter 叠加为 AND；QIF-9）
    pub kind: String,
    /// 仅列出测试（--list），不执行
    pub list_only: bool,
    /// 列出测试的输出格式：text | json（QIF-8）
    pub list_format: String,
    /// RFC 034 Phase 4: 启用并行测试执行（对标 XUnit 默认行为）
    pub parallel: bool,
    /// 并行执行的最大并发度（0 表示未启用/串行；>1 时并行；-1 表示不限）。
    /// 由 `arc.toml [qif].max_parallel` + CLI `--max-parallel` 合并而来。
    pub max_parallel: i32,
    /// 默认单测试超时毫秒（0 = 不限制）。由 `arc.toml [qif].default_timeout` 合并而来。
    pub default_timeout_ms: i32,
    /// RFC 032 §7：报告产物根目录（`.arcqif` + `report.json`），已解析为绝对路径。
    pub output_dir: String,
    /// RFC 032 §7：是否生成 `report.json`（[qif].emit_json_report，默认 true）。
    pub emit_json_report: bool,
    /// RFC 032 §7：是否持久化 `.arcqif`（[qif].persist_results，默认 true）。
    pub persist_results: bool,
}

/// 编译选项（RFC 036 §2.5 / RFC 005 §2.3）。
///
/// `nll_strict`：NLL 借用检查，**恒启用**（RFC 005 立宪无条件启用，无逃生舱；
/// 原兼容显式 on / 逃生舱开关均已收敛，⑤ 已移除 CLI flag）。
/// 启用时 `prepare_compilation` 在 MIR 生成后运行 `run_nll_check_module`，
/// 非空诊断 → 编译失败。
#[derive(Clone, Debug)]
pub struct CompileOptions {
    /// RFC 036 §2.5：NLL 借用检查（默认 on；RFC 005 后恒启用）。
    pub nll_strict: bool,
    /// RFC 005 里程碑④：编译期声明级字段环 warning 策略（`arc-cycle-001`）。
    /// 默认 `warn`（打印到 stderr 不阻断编译）；`off` 完全静默。
    /// **无 `error` 档**——声明级环不必然泄漏，永不当 error（RFC 005 §2.6 / §5）。
    pub field_cycle_policy: FieldCyclePolicy,
    /// RFC 017 产物域（U3 .ll 焚毁，UX 迭代评审 §2.3）：clang 编译成功后是否保留
    /// 文本 IR。默认 `false`（焚毁，杜绝 obj/ 内 .ll 膨胀）；`--emit-llvm` 显式
    /// 置 true 供 IR 诊断。clang 失败路径恒保留现场，不受此开关影响。
    pub keep_ir: bool,
}

/// RFC 005 里程碑④：`[compiler] field_cycle_policy` 策略旋钮（warning-by-default）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FieldCyclePolicy {
    /// 默认：`arc-cycle-001` warning 打印到 stderr，不阻断编译（exit 0）。
    #[default]
    Warn,
    /// 完全静默：不打印 warning（typeck 仍收集，pipeline 过滤）。
    Off,
}

impl FieldCyclePolicy {
    /// 解析策略字符串：`"warn"` | `"off"`。未知值返回 `None`。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "warn" => Some(Self::Warn),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Off => "off",
        }
    }
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            nll_strict: true,
            field_cycle_policy: FieldCyclePolicy::Warn,
            keep_ir: false,
        }
    }
}

pub fn compile_file(
    file_path: &Path,
    release: bool,
    debug_info: bool,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    project_kind: codegen::ProjectKind,
    options: &CompileOptions,
) -> Result<(), String> {
    compile_file_with_native(
        file_path,
        release,
        debug_info,
        output,
        obj_dir,
        target,
        &[],
        project_kind,
        options,
    )
}

/// `compile_file` + native 库搜索路径（RFC 016 M2）。
///
/// `native_lib_paths` 会注入为链接器 `-L<DIR>` 标志。
pub fn compile_file_with_native(
    file_path: &Path,
    release: bool,
    debug_info: bool,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    native_lib_paths: &[PathBuf],
    project_kind: codegen::ProjectKind,
    options: &CompileOptions,
) -> Result<(), String> {
    let mut unit = load_compile_unit(file_path).map_err(|e| format!("load error: {e}"))?;
    // For directory projects (multi-file), the source is synthesized; pass empty string.
    let source = if file_path.is_dir() {
        String::new()
    } else {
        std::fs::read_to_string(file_path).map_err(|e| format!("read source error: {e}"))?
    };
    let equipment = Equipments::new();
    compile_unit(
        &mut unit,
        &source,
        file_path,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_lib_paths,
        project_kind,
        options,
        &*equipment.emitter,
    )
}

/// RFC 017 D8 v1.0：编译为动态库（`.dll`/`.so`/`.dylib`）。
///
/// 与 [`compile_file_with_native`] 平行的入口，产物为动态库。对齐 C# 程序集
/// 模型——动态库 = 干净的库逻辑 + 引用链接信息。
///
/// # 与 `compile_file_with_native` 的差异
///
/// - **不要求 `main` 函数**：动态库无入口点，领域约定符号由 host 按需查找
/// - **使用 `compile_module_to_dynamic_library`**（`-shared` + `-fPIC`）
/// - **`export_symbols`**：领域约定符号列表（如 `__qif_init`），Windows MSVC
///   下转换为 `/EXPORT:<symbol>` 显式导出
///
/// # 典型用法
///
/// `arc build --dynamic` 或 `arc build`（manifest `kind="library"` + `dynamic=true`）。
pub fn compile_file_to_dynamic_library(
    file_path: &Path,
    release: bool,
    debug_info: bool,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    native_lib_paths: &[PathBuf],
    export_symbols: &[String],
    package_meta: Option<codegen::PackageMeta>,
    options: &CompileOptions,
) -> Result<(), String> {
    let mut unit = load_compile_unit(file_path).map_err(|e| format!("load error: {e}"))?;
    let source = if file_path.is_dir() {
        String::new()
    } else {
        std::fs::read_to_string(file_path).map_err(|e| format!("read source error: {e}"))?
    };
    let equipment = Equipments::new();
    compile_unit_to_dynamic_library(
        &mut unit,
        &source,
        file_path,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_lib_paths,
        export_symbols,
        package_meta,
        options,
        &*equipment.emitter,
    )
}

pub fn compile_source(
    source: &str,
    file_path: &Path,
    release: bool,
    debug_info: bool,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    project_kind: codegen::ProjectKind,
    options: &CompileOptions,
) -> Result<(), String> {
    compile_source_with_native(
        source,
        file_path,
        release,
        debug_info,
        output,
        obj_dir,
        target,
        &[],
        project_kind,
        options,
    )
}

/// `compile_source` + native 库搜索路径（RFC 016 M2）。
pub fn compile_source_with_native(
    source: &str,
    file_path: &Path,
    release: bool,
    debug_info: bool,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    native_lib_paths: &[PathBuf],
    project_kind: codegen::ProjectKind,
    options: &CompileOptions,
) -> Result<(), String> {
    let mut file_registry = FileRegistry::new();
    let workspace = crate::loader::find_workspace_root(file_path);
    let native_modules = crate::loader::load_native_contracts(&workspace, &mut file_registry)
        .map_err(|e| format!("load native contracts error: {e}"))?;

    let file_id = file_registry.allocate(file_path.to_path_buf());
    let program = parse::Parser::parse_program_in_file(source, file_id).map_err(|e| {
        emit_parse_error(source, file_path, &e);
        format!("parse error: {e}")
    })?;

    let mut unit = CompileUnit {
        program,
        root: file_path.to_path_buf(),
        file_registry,
        native_modules,
        external_symbols: Vec::new(),
        file_packages: {
            let mut m = std::collections::HashMap::new();
            m.insert(file_id, "App".to_string());
            m
        },
        internals_visible_to: std::collections::HashMap::new(),
        entry_package: "App".to_string(),
    };
    let equipment = Equipments::new();
    compile_unit(
        &mut unit,
        source,
        file_path,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_lib_paths,
        project_kind,
        options,
        &*equipment.emitter,
    )
}

/// RFC 027 ResX CodeGen：扫描源目录 `.resx` → 生成强类型访问器源码 → 注入编译单元。
///
/// `Messages.resx`(+`Messages.zh-CN.resx`) → 顶层 `Messages` 类（静态属性），
/// 经常规流水线下沉为字面量/静态属性——运行时零解析、零哈希、零 ABI 调用。
/// 文化文件命名 `<Base>.<Culture>.resx`；文化 key 缺失于 neutral → 硬错误。
fn maybe_inject_resx_accessors(
    unit: &mut CompileUnit,
    source_file: &Path,
    obj_dir: Option<&Path>,
) -> Result<(), String> {
    let source_dir = match source_file.parent() {
        Some(dir) => dir,
        None => return Ok(()),
    };

    let entries = match std::fs::read_dir(source_dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(()),
    };

    // (base, culture?) → 资源集分组（保持目录迭代顺序稳定的文件名排序）
    let mut files: Vec<(String, Option<String>, String)> = Vec::new(); // (base, culture, xml)
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if ext.to_lowercase() != "resx" {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if stem.is_empty() {
            continue;
        }
        let xml = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {} error: {e}", path.display()))?;
        let (base, culture) = codegen::resx_compiler::split_resx_stem(&stem);
        files.push((base, culture, xml));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // 分组：base → (neutral, cultures)
    let mut neutral: Vec<(String, codegen::resx_compiler::ResResourceSet)> = Vec::new();
    let mut cultures: Vec<(String, String, codegen::resx_compiler::ResResourceSet)> = Vec::new();
    for (base, culture, xml) in &files {
        let set = codegen::resx_compiler::parse_resx(xml).map_err(|e| format!("{}: {e}", base))?;
        match culture {
            None => neutral.push((base.clone(), set)),
            Some(c) => cultures.push((base.clone(), c.clone(), set)),
        }
    }

    // 文化文件必须有对应 neutral（否则 key 无兜底家）
    for (base, culture, _) in &cultures {
        if !neutral.iter().any(|(b, _)| b == base) {
            return Err(format!(
                "R054025: '{base}.{culture}.resx' has no neutral '{base}.resx' (neutral file is required)"
            ));
        }
    }

    // 逐组生成访问器并注入
    for (base, neutral_set) in &neutral {
        let group_cultures: Vec<(String, codegen::resx_compiler::ResResourceSet)> = cultures
            .iter()
            .filter(|(b, _, _)| b == base)
            .map(|(_, c, s)| (c.clone(), s.clone()))
            .collect();
        let group = codegen::resx_compiler::ResxGroup {
            base_name: base.clone(),
            neutral: neutral_set.clone(),
            cultures: group_cultures,
        };
        let gen_source =
            codegen::resx_compiler::generate_accessor_source(&group).map_err(|e| format!("{e}"))?;

        let class_name = group.class_name().to_string();
        let gen_path = obj_dir
            .unwrap_or_else(|| Path::new("obj"))
            .join("code")
            .join(format!("resx_{class_name}.g.as"));
        let gen_file_id = unit.file_registry.allocate(gen_path);
        let gen_program =
            parse::Parser::parse_program_in_file(&gen_source, gen_file_id).map_err(|e| {
                format!(
                    "parse generated resx accessor: {e}\n\n-- generated source --\n{gen_source}"
                )
            })?;
        unit.program.items.extend(gen_program.items);
    }

    Ok(())
}

fn compile_unit(
    unit: &mut CompileUnit,
    source: &str,
    source_file: &Path,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    release: bool,
    debug_info: bool,
    native_lib_paths: &[PathBuf],
    project_kind: codegen::ProjectKind,
    options: &CompileOptions,
    emitter: &dyn ArtifactEmitter,
) -> Result<(), String> {
    // RFC 038/012：声明式 [AITool] + [Inject] → 编译期合成静态注册并入程序（普通 build / test 均生效）。
    maybe_inject_runtime_registries(unit, obj_dir)?;
    // RFC 027 ResX CodeGen：.resx → 强类型访问器注入（普通 build / test 均生效）。
    maybe_inject_resx_accessors(unit, source_file, obj_dir)?;
    let prepared = prepare_compilation(unit, options, false)?;

    if let Some(out) = output {
        let triple = target.map(|t| t.as_str());
        // 产物发射（P5）走装备接口：亲 `codegen::compile_module*` 由默认装备委托。
        let diags = emitter
            .emit(ArtifactRequest {
                role: EmitRole::MainObject,
                fns: &prepared.mir_fns,
                layouts: &prepared.layouts,
                output: out,
                obj_dir,
                target: triple,
                release,
                file_path: &prepared.file_path,
                source,
                debug_info,
                fn_spans: &prepared.fn_spans,
                native_modules: &prepared.native_modules,
                native_lib_paths,
                external_symbols: &prepared.external_symbols,
                project_kind,
                export_symbols: &[],
                package_meta: None,
                keep_ir: options.keep_ir,
            })
            .map_err(|e| format!("codegen error: {e}"))?;
        render_static_init_diagnostics(&diags);
    }

    emit_doc_xml(unit, output, obj_dir)?;
    Ok(())
}

/// 渲染 codegen 静态初始化依赖分析诊断（`arc-sinit-001/002`）到 stderr。
///
/// 与 typeck `arc-cycle-001` warning 共用 `warning[<code>]: <message>` 渲染约定；
/// 不阻断编译（exit 0）。无诊断时静默（正常项目零噪声）。
fn render_static_init_diagnostics(diags: &[codegen::StaticInitDiagnostic]) {
    for d in diags {
        eprintln!("{}", d.render());
    }
}

/// 加载测试编译单元：无 `.aopkg` 依赖走普通源码加载；有依赖时经
/// [`load_compile_unit_with_aopkg_deps`] 把依赖包 exports 注册为外部符号
///（`--dep` 预编译包可被测试项目 `using` 消费，对齐 `arc build --dep`）。
fn load_test_unit(root: &Path) -> Result<CompileUnit, String> {
    load_compile_unit(root).map_err(|e| format!("load error: {e}"))
}

/// 测试编译收尾：走单模块 [`compile_unit`]。
fn compile_test_unit(
    unit: &mut CompileUnit,
    source: &str,
    root: &Path,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    release: bool,
    debug_info: bool,
    native_lib_paths: &[PathBuf],
    options: &CompileOptions,
) -> Result<(), String> {
    let equipment = Equipments::new();
    compile_unit(
        unit,
        source,
        root,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_lib_paths,
        codegen::ProjectKind::Executable,
        options,
        &*equipment.emitter,
    )
}

/// RFC 032 Phase 2c: 编译 QIF 测试文件为可执行文件（纯 Arc 测试链路）。
///
/// 与 [`compile_file`] 的区别：
/// 1. 扫描 AST 收集 [Fact]/[Theory] 方法（含 Order / DisplayName / ctor params）
/// 2. 按 Order 排序，应用 filter 过滤，支持 --list 模式
/// 3. 生成合成 `__QifTestHost.Main()` 入口函数（含构造函数注入 + QIFOptions）
/// 4. 注入到编译单元后正常编译
///
/// 产物为可执行文件，可直接运行输出测试结果。
pub fn compile_test_file(
    file_path: &Path,
    release: bool,
    debug_info: bool,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    native_lib_paths: &[PathBuf],
    qif_opts: &QifCompileOptions,
    options: &CompileOptions,
) -> Result<(), String> {
    let mut unit = load_test_unit(file_path)?;
    // For directory projects (multi-file), the source is synthesized; pass empty string.
    let source = if file_path.is_dir() {
        String::new()
    } else {
        std::fs::read_to_string(file_path).map_err(|e| format!("read source error: {e}"))?
    };

    // 1. 从 AST 收集 [Fact]/[Theory] 方法
    let qif_methods = collect_qif_test_methods(&unit.program);

    // 1b. 应用 QIF-6/9 过滤（filter + namespace + kind 三者 AND）
    let qif_methods = if qif_opts.filter.is_empty()
        && qif_opts.namespace.is_empty()
        && qif_opts.kind.is_empty()
    {
        qif_methods
    } else {
        apply_qif_filter(
            qif_methods,
            &qif_opts.filter,
            &qif_opts.namespace,
            &qif_opts.kind,
        )?
    };

    if qif_methods.is_empty() {
        // 无测试方法，走正常编译路径（测试代码始终为可执行程序）
        return compile_test_unit(
            &mut unit,
            &source,
            file_path,
            output,
            obj_dir,
            target,
            release,
            debug_info,
            native_lib_paths,
            options,
        );
    }

    // K4：测试模式入口由合成 __QifTestHost::Main 接管——剔除用户顶层 Main，
    // 避免 `multiple main functions found: Main, __QifTestHost::Main`。
    strip_entry_main(&mut unit.program.items);

    // 2. 生成合成 __QifTestHost.Main()（含 Order 排序 + filter + QIFOptions）——测试宿主合成（P6）走装备接口。
    let equipment = Equipments::new();
    let gen_source = equipment.host.generate(&qif_methods, qif_opts);

    // 2b. --list 模式：仅输出测试列表到 stdout，不编译
    if qif_opts.list_only {
        // QIF-8：按 list_format 选择 text（默认）或 json 输出
        if qif_opts.list_format == "json" {
            print_list_json(&qif_methods);
        } else {
            // 稳定字典序排序 + 统一字段顺序，保证 list 输出可 diff 可回归
            let mut sorted: Vec<&QifTestMethod> = qif_methods.iter().collect();
            sorted.sort_by(|a, b| {
                let da = display_of(a);
                let db = display_of(b);
                da.cmp(&db)
            });
            for m in &sorted {
                let display = display_of(m);
                let coll = if let Some(ref c) = m.collection_name {
                    format!(" [Collection: {c}]")
                } else {
                    String::new()
                };
                let skip = if let Some(ref s) = m.skip_reason {
                    format!(" [SKIP: {s}]")
                } else {
                    String::new()
                };
                let traits = if m.traits.is_empty() {
                    String::new()
                } else {
                    let ts: Vec<String> =
                        m.traits.iter().map(|(k, v)| format!("{k}:{v}")).collect();
                    format!(" [{}]", ts.join(", "))
                };
                println!(
                    "[{}] {}{}{}{} (Order={})",
                    m.attr_name, display, skip, coll, traits, m.order
                );
            }
            println!("Total: {} test methods", sorted.len());
        }
        return Ok(());
    }

    // 3. 分配合成文件 ID 并解析（.NET 体系：生成代码落入 obj/<config>/code/）
    let gen_path = obj_dir
        .unwrap_or_else(|| Path::new("obj"))
        .join("code")
        .join("__qif_test_main.g.as");
    let gen_file_id = unit.file_registry.allocate(gen_path);

    let gen_program =
        parse::Parser::parse_program_in_file(&gen_source, gen_file_id).map_err(|e| {
            format!("parse generated test main: {e}\n\n-- generated source --\n{gen_source}")
        })?;

    // 4. 合并到用户程序
    unit.program.items.extend(gen_program.items);

    // 5. 正常编译（测试代码始终为可执行程序）
    compile_test_unit(
        &mut unit,
        &source,
        file_path,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_lib_paths,
        options,
    )
}

/// 项目级测试编译——对标 `dotnet test`：扫描项目目录下所有 `.as` 文件，
/// 合并为一个 [`CompileUnit`]，全局收集 `[Fact]`/`[Theory]` 方法并生成合成
/// `__QifTestHost.Main()` 入口。
pub fn compile_test_project(
    project_root: &Path,
    release: bool,
    debug_info: bool,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    native_lib_paths: &[PathBuf],
    qif_opts: &QifCompileOptions,
    options: &CompileOptions,
) -> Result<(), String> {
    // 1. 加载项目中所有 .as 文件（带 `.aopkg` 依赖时经外部符号路径加载，
    // 使测试项目可消费 `--dep` 预编译包，对齐 `arc build --dep`）。
    let mut unit = load_test_unit(project_root)?;

    // 2. 从合并后的 AST 收集所有 [Fact]/[Theory] 方法
    let qif_methods = collect_qif_test_methods(&unit.program);

    // 2b. 应用 QIF-6/9 过滤（filter + namespace + kind 三者 AND）
    let qif_methods = if qif_opts.filter.is_empty()
        && qif_opts.namespace.is_empty()
        && qif_opts.kind.is_empty()
    {
        qif_methods
    } else {
        apply_qif_filter(
            qif_methods,
            &qif_opts.filter,
            &qif_opts.namespace,
            &qif_opts.kind,
        )?
    };

    if qif_methods.is_empty() {
        return Err("no test methods ([Fact]/[Theory]) found in project".to_string());
    }

    // K4：测试模式入口由合成 __QifTestHost::Main 接管——剔除用户顶层 Main，
    // 避免与合成入口冲突（含 Main 的 app 项目 `arc test` 不再 multiple main）。
    strip_entry_main(&mut unit.program.items);

    // 3. 生成合成 __QifTestHost.Main()——测试宿主合成（P6）走装备接口。
    let equipment = Equipments::new();
    let gen_source = equipment.host.generate(&qif_methods, qif_opts);

    // 3b. --list 模式：仅输出测试列表
    if qif_opts.list_only {
        // QIF-8：按 list_format 选择 text/json
        if qif_opts.list_format == "json" {
            print_list_json(&qif_methods);
        } else {
            let mut sorted: Vec<&QifTestMethod> = qif_methods.iter().collect();
            sorted.sort_by_key(|a| display_of(a));
            for m in &sorted {
                let display = display_of(m);
                let coll = if let Some(ref c) = m.collection_name {
                    format!(" [Collection: {c}]")
                } else {
                    String::new()
                };
                let skip = if let Some(ref s) = m.skip_reason {
                    format!(" [SKIP: {s}]")
                } else {
                    String::new()
                };
                let traits = if m.traits.is_empty() {
                    String::new()
                } else {
                    let ts: Vec<String> =
                        m.traits.iter().map(|(k, v)| format!("{k}:{v}")).collect();
                    format!(" [{}]", ts.join(", "))
                };
                println!(
                    "[{}] {}{}{}{} (Order={})",
                    m.attr_name, display, skip, coll, traits, m.order
                );
            }
            println!("Total: {} test methods", sorted.len());
        }
        return Ok(());
    }

    // 4. 分配合成文件 ID 并解析
    let gen_path = obj_dir
        .unwrap_or_else(|| Path::new("obj"))
        .join("code")
        .join("__qif_test_main.g.as");
    let gen_file_id = unit.file_registry.allocate(gen_path);

    let gen_program =
        parse::Parser::parse_program_in_file(&gen_source, gen_file_id).map_err(|e| {
            format!("parse generated test main: {e}\n\n-- generated source --\n{gen_source}")
        })?;

    // 5. 合并到用户程序
    unit.program.items.extend(gen_program.items);

    // 6. 编译所有源文件（合并视图用于 debug info）
    let combined_source = String::new(); // 多文件项目无单一 source

    compile_test_unit(
        &mut unit,
        &combined_source,
        project_root,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        native_lib_paths,
        options,
    )
}

/// 判断项目目录是否包含至少一个 `[Fact]`/`[Theory]` 测试方法。
///
/// 供 workspace `arc test` 聚合使用：无测试成员的 [`compile_test_project`] 会以
/// 「no test methods」硬错误中断聚合，故先据此判别、跳过非测试成员。
pub fn project_has_tests(project_root: &Path) -> Result<bool, String> {
    let unit = load_compile_unit_from_dir(project_root).map_err(|e| format!("load error: {e}"))?;
    Ok(!collect_qif_test_methods(&unit.program).is_empty())
}

/// K4：`arc test` 下剔除用户顶层 `Main` 自由函数（含 file-scoped/block
/// namespace 内嵌套的自由函数 `Main`），避免与合成 `__QifTestHost::Main`
/// 入口冲突——测试模式下程序入口由测试宿主接管，用户 `Main` 不再作为入口
/// （对标 `dotnet test` 的入口接管）。类方法 `Main` 不受影响（非入口）。
fn strip_entry_main(items: &mut Vec<ast::Spanned<Item>>) {
    items.retain_mut(|item| match &mut item.node {
        ast::Item::Namespace(ns) => {
            strip_entry_main(&mut ns.items);
            true
        }
        ast::Item::Fn(f) if f.name.as_str() == "Main" => false,
        _ => true,
    });
}

/// RFC 032 Phase 2c: AST 遍历收集 [Fact]/[Theory] 标记的测试方法。
///
/// 同时收集 [Order(N)]、DisplayName、构造函数参数类型。
/// 结果按 Order 升序排序（P4）。
fn collect_qif_test_methods(program: &ast::Program) -> Vec<QifTestMethod> {
    let mut methods = Vec::new();
    collect_from_items(&program.items, &mut methods, "");
    methods.sort_by_key(|m| m.order);
    methods
}

fn collect_from_items(
    items: &[ast::Spanned<ast::Item>],
    out: &mut Vec<QifTestMethod>,
    ns_prefix: &str,
) {
    for spanned in items {
        match &spanned.node {
            ast::Item::Class(c) => {
                // 收集构造函数参数类型（P2/P3：构造函数注入 / IClassFixture）
                let mut ctor_param_types: Vec<String> = Vec::new();
                for ctor in &c.constructors {
                    for param in &ctor.node.params {
                        ctor_param_types.push(type_to_string(&param.ty.node));
                    }
                }

                // 收集类级 [Collection("name")] 属性
                let collection_name: Option<String> = c.attributes.iter().find_map(|attr| {
                    if attr.path.first().map(|i| i.to_string()).unwrap_or_default() == "Collection"
                    {
                        attr.args.first().and_then(|a| {
                            if let ast::AttributeArg::String(s) = a {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                });

                // 收集类级 [Trait("name", "value")] 属性
                let class_traits: Vec<(String, String)> = extract_traits(&c.attributes);

                for method in &c.methods {
                    let mut is_fact = false;
                    let mut is_theory = false;
                    let mut inline_data: Vec<Vec<QifInlineArg>> = Vec::new();
                    let mut order: i32 = 0;
                    let mut display_name = String::new();
                    let mut skip_reason: Option<String> = None;

                    // 收集方法级 [Trait]
                    let method_traits = extract_traits(&method.node.sig.attributes);
                    // 合并类级 + 方法级 traits
                    let mut all_traits = class_traits.clone();
                    all_traits.extend(method_traits);

                    for attr in &method.node.sig.attributes {
                        let name = attr.path.first().map(|i| i.to_string()).unwrap_or_default();
                        match name.as_str() {
                            "Fact" => {
                                is_fact = true;
                                if let Some(v) = extract_named_arg(attr, "DisplayName") {
                                    display_name = v;
                                }
                                skip_reason = extract_named_arg(attr, "Skip");
                            }
                            "Theory" => {
                                is_theory = true;
                                if let Some(v) = extract_named_arg(attr, "DisplayName") {
                                    display_name = v;
                                }
                                skip_reason = extract_named_arg(attr, "Skip");
                            }
                            "InlineData" => {
                                let args: Vec<QifInlineArg> = attr
                                    .args
                                    .iter()
                                    .filter_map(|a| match a {
                                        ast::AttributeArg::Int(n) => Some(QifInlineArg::Int(*n)),
                                        ast::AttributeArg::String(s) => {
                                            Some(QifInlineArg::String(s.clone()))
                                        }
                                        ast::AttributeArg::Bool(b) => Some(QifInlineArg::Bool(*b)),
                                        _ => None,
                                    })
                                    .collect();
                                if !args.is_empty() {
                                    inline_data.push(args);
                                }
                            }
                            "Order" => {
                                if let Some(ast::AttributeArg::Int(n)) = attr.args.first() {
                                    order = *n as i32;
                                }
                            }
                            _ => {}
                        }
                    }

                    let attr_name = if is_fact {
                        "Fact".to_string()
                    } else if is_theory {
                        "Theory".to_string()
                    } else {
                        continue;
                    };

                    out.push(QifTestMethod {
                        class_name: c.name.to_string(),
                        method_name: method.node.sig.name.to_string(),
                        attr_name,
                        inline_data,
                        order,
                        display_name,
                        ctor_param_types: ctor_param_types.clone(),
                        is_async: method.node.sig.is_async,
                        collection_name: collection_name.clone(),
                        skip_reason,
                        traits: all_traits,
                        namespace: ns_prefix.to_string(),
                    });
                }
            }
            ast::Item::Namespace(ns) => {
                let ns_name: String = ns
                    .path
                    .iter()
                    .map(|i| i.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                let child_ns = if ns_prefix.is_empty() {
                    ns_name
                } else {
                    format!("{}.{}", ns_prefix, ns_name)
                };
                collect_from_items(&ns.items, out, &child_ns);
            }
            _ => {}
        }
    }
}

/// 将 AST Type 节点转换为 Arc 类型名字符串。
fn type_to_string(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Named { path, generics } => {
            let name = path.last().map(|i| i.to_string()).unwrap_or_default();
            if generics.is_empty() {
                name
            } else {
                let args: Vec<String> = generics.iter().map(|g| type_to_string(&g.node)).collect();
                format!("{name}<{}>", args.join(", "))
            }
        }
        ast::Type::Array { inner } => format!("{}[]", type_to_string(&inner.node)),
        ast::Type::Nullable { inner } => format!("{}?", type_to_string(&inner.node)),
        ast::Type::Ref { inner, .. } => type_to_string(&inner.node),
        ast::Type::Func { .. } => "Action".to_string(),
        ast::Type::ConstInt(_) => "int".to_string(),
        ast::Type::Infer => "var".to_string(),
    }
}

/// 将 AST `Type` 转换为 `TypeId`（对齐 codegen `type_id_from_name`；供
/// `static_init_new_class` 对泛型实参单态化）。
fn ast_type_to_type_id(ty: &ast::Type) -> typeck::TypeId {
    match ty {
        ast::Type::Named { path, .. } => {
            let name = path.last().map(|i| i.to_string()).unwrap_or_default();
            match name.as_str() {
                "int" => typeck::TypeId::Int,
                "long" => typeck::TypeId::Long,
                "short" => typeck::TypeId::Short,
                "byte" => typeck::TypeId::Byte,
                "uint" => typeck::TypeId::UInt,
                "ushort" => typeck::TypeId::UShort,
                "sbyte" => typeck::TypeId::SByte,
                "char" => typeck::TypeId::Char,
                "bool" => typeck::TypeId::Bool,
                "float" => typeck::TypeId::Float,
                "double" => typeck::TypeId::Double,
                "string" => typeck::TypeId::String,
                "object" => typeck::TypeId::Object,
                "void" => typeck::TypeId::Void,
                other => typeck::TypeId::Named(other.into()),
            }
        }
        _ => typeck::TypeId::Void,
    }
}

/// 计算静态初始化器 `new T(...)` 的目标单态化类名，与 codegen `emit_static_new_expr`
/// 完全一致：非泛型取 `path.last()`；泛型经 `mangle_generic` 还原（如
/// `new Dictionary<A,B>()` → `Dictionary_A_B`）。供 tree-shake force-keep ctor 用。
fn static_init_new_class(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Named { path, generics } => {
            let def = path.last().map(|i| i.to_string()).unwrap_or_default();
            if generics.is_empty() {
                def
            } else {
                let args: Vec<typeck::TypeId> = generics
                    .iter()
                    .map(|g| ast_type_to_type_id(&g.node))
                    .collect();
                typeck::mangle_generic(&def, &args)
            }
        }
        _ => String::new(),
    }
}

fn extract_named_arg(attr: &ast::Attribute, name: &str) -> Option<String> {
    for arg in &attr.args {
        if let ast::AttributeArg::Named { name: n, value } = arg {
            if n.as_str() == name {
                if let ast::AttributeArg::String(s) = value.as_ref() {
                    return Some(s.clone());
                }
            }
        }
    }
    None
}

/// 从属性列表中提取 [Trait("name", "value")]。
fn extract_traits(attrs: &[ast::Attribute]) -> Vec<(String, String)> {
    attrs
        .iter()
        .filter_map(|attr| {
            if attr.path.first().map(|i| i.to_string()).unwrap_or_default() == "Trait" {
                let name = attr.args.first().and_then(|a| {
                    if let ast::AttributeArg::String(s) = a {
                        Some(s.clone())
                    } else {
                        None
                    }
                })?;
                let value = attr.args.get(1).and_then(|a| {
                    if let ast::AttributeArg::String(s) = a {
                        Some(s.clone())
                    } else {
                        None
                    }
                })?;
                Some((name, value))
            } else {
                None
            }
        })
        .collect()
}

/// 将运行期字符串安全嵌入 Arc 字符串字面量：转义反斜杠与双引号，防宿主路径/名称破坏语法。
fn escape_arc_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// RFC 032 Phase 2c: 生成合成 __QifTestHost.Main() 的 Arc 源码。
///
/// 生成的 Main 函数：
/// - 使用 QIFHost(QIFOptions) 配置报告格式
/// - 按 Order 排序后执行（已在 collect 阶段排序）
/// - 注：过滤已在方法收集阶段由 `apply_qif_filter` 完成；本函数只负责代码生成
/// - 支持 DisplayName 自定义测试显示名
/// - P2/P3：构造函数注入——对有参构造函数的测试类生成参数构造代码
/// - 对每个 [Fact]/[Theory] 方法：try-catch，IQIFSetup/IQIFTeardown 生命周期
/// - 根据 output_format 选择 Report / ReportJson / ReportJUnit
/// - 含 `async` Fact/Theory 时：Main/RunBatch 为 async，调用处 `await`（EventLoop 驱动）；
///   异步套件强制串行（Parallel.For 无法 await），失败时 `Environment.Exit(1)`
///   （async Main 的 EventLoop wrapper 固定 `ret i32 0`）
///
/// RFC 032 §7：Reporting 后、`Environment.Exit` 前写入 `report.json` / `.arcqif` 落盘产物。
pub(crate) fn generate_qif_test_main(
    methods: &[QifTestMethod],
    qif_opts: &QifCompileOptions,
) -> String {
    const BATCH_SIZE: usize = 50;

    let mut out = String::new();

    // --- 方法列表已在 `apply_qif_filter` 阶段过滤完成；此处直接引用 ---
    let filtered: Vec<&QifTestMethod> = methods.iter().collect();

    let has_async = filtered.iter().any(|m| m.is_async);

    // --- 收集需要 IClassFixture 构造注入的类 ---
    let ctor_map: std::collections::HashMap<String, Vec<String>> = {
        let mut map = std::collections::HashMap::new();
        for m in &filtered {
            if !m.ctor_param_types.is_empty() {
                map.entry(m.class_name.clone())
                    .or_insert_with(|| m.ctor_param_types.clone());
            }
        }
        map
    };

    // --- 标记哪些类有 IQIFOutput ctor 参数 ---
    let output_fixtures: std::collections::HashMap<String, usize> = ctor_map
        .iter()
        .filter_map(|(cls, params)| {
            params
                .iter()
                .position(|p| p == "IQIFOutput")
                .map(|i| (cls.clone(), i))
        })
        .collect();

    // --- 收集类级 [Collection("name")] 元数据 ---
    let collection_map: std::collections::HashMap<String, String> = filtered
        .iter()
        .filter_map(|m| {
            m.collection_name
                .as_ref()
                .map(|c| (m.class_name.clone(), c.clone()))
        })
        .collect();

    // --- 构建方法级 traits 字符串 ---
    let method_traits_map: std::collections::HashMap<(String, String), String> = filtered
        .iter()
        .filter(|m| !m.traits.is_empty())
        .map(|m| {
            let ts: Vec<String> = m.traits.iter().map(|(k, v)| format!("{k}:{v}")).collect();
            ((m.class_name.clone(), m.method_name.clone()), ts.join(";"))
        })
        .collect();

    let get_traits = |cls: &str, method: &str| -> &str {
        method_traits_map
            .get(&(cls.to_string(), method.to_string()))
            .map(|s| s.as_str())
            .unwrap_or("")
    };
    let get_coll_trait = |cls: &str| -> String {
        collection_map
            .get(cls)
            .map(|c| format!("Collection:{c}"))
            .unwrap_or_default()
    };
    let build_traits_str = |cls: &str, method: &str| -> String {
        let ct = get_coll_trait(cls);
        let mt = get_traits(cls, method);
        if ct.is_empty() {
            mt.to_string()
        } else if mt.is_empty() {
            ct
        } else {
            format!("{ct};{mt}")
        }
    };

    // --- 辅助：为单个测试方法生成执行代码 ---
    let emit_test_call = |out: &mut String,
                          method: &QifTestMethod,
                          _method_idx: usize,
                          _batch_offset: usize| {
        let display_name = if method.display_name.is_empty() {
            format!("{}.{}", method.class_name, method.method_name)
        } else {
            method.display_name.clone()
        };
        let cls = &method.class_name;

        let new_expr = if let Some(params) = ctor_map.get(cls.as_str()) {
            let args: Vec<String> = params
                .iter()
                .enumerate()
                .map(|(i, _p)| format!("this._fixture_{}_{}", cls, i))
                .collect();
            if args.is_empty() {
                format!("new {cls}()")
            } else {
                format!("new {cls}({})", args.join(", "))
            }
        } else {
            format!("new {cls}()")
        };

        let traits_str = build_traits_str(cls, &method.method_name);
        let has_traits = !traits_str.is_empty();
        let (rp_method, rs_method) = if has_traits {
            ("RecordPassT", "RecordSkipT")
        } else {
            ("RecordPass", "RecordSkip")
        };
        let trait_param = if has_traits {
            format!(", \"{traits_str}\"")
        } else {
            String::new()
        };

        // 超时失败统一走 traits 感知的 fail 方法（无 traits 时回退 RecordFail）。
        let fail_method = if has_traits {
            "RecordFailT"
        } else {
            "RecordFail"
        };
        let fail_trait_param = if has_traits {
            format!(", \"{traits_str}\"")
        } else {
            String::new()
        };

        let timeout_enabled = qif_opts.default_timeout_ms > 0;
        let timeout_ms_expr = "this.runner.DefaultTimeoutMs";
        let dur_ns = "_sw.ElapsedTicks * 1000000000 / Stopwatch.Frequency";

        // 生成"方法调用 + 计时 + 通过/超时判定"段。
        let emit_invoke = |out: &mut String, display: &str, kind: &str, args_str: &str| {
            let method_call = if args_str.is_empty() {
                format!("_obj.{}()", method.method_name)
            } else {
                format!("_obj.{}({})", method.method_name, args_str)
            };
            let pass_record = format!(
                "            this.runner.{rp_method}(\"{display}\", QIFTestKind.{kind}, _durNs{trait_param});\n"
            );
            let timeout_record = format!(
                "            this.runner.{fail_method}(\"{display}\", QIFTestKind.{kind}, _durNs, \"Timeout after \" + {timeout_ms_expr}.ToString() + \"ms\"{fail_trait_param});\n"
            );
            if method.is_async && timeout_enabled {
                // 异步超时：Task.WhenAny 真实唤醒（WhenAny 返回 Task<Void>，不返回胜者 inner；
                // 故 await 后以 _testTask.IsCompleted 判定哪个先完成——测试先完成记通过，
                // delay 先完成记超时；测试任务后台继续，结果按超时计）。
                out.push_str(&format!(
                    "                Task _testTask = {method_call};\n"
                ));
                out.push_str(&format!(
                    "                Task _delay = Task.Delay({timeout_ms_expr});\n"
                ));
                out.push_str("                await Task.WhenAny(_testTask, _delay);\n");
                out.push_str("                _sw.Stop();\n");
                out.push_str(&format!("                long _durNs = {dur_ns};\n"));
                out.push_str("                if (_testTask.IsCompleted) {\n");
                out.push_str("                    await _testTask;\n");
                out.push_str(&pass_record);
                out.push_str("                } else {\n");
                out.push_str(&timeout_record);
                out.push_str("                }\n");
            } else {
                if method.is_async {
                    out.push_str(&format!("                await {method_call};\n"));
                } else {
                    out.push_str(&format!("                {method_call};\n"));
                }
                out.push_str("                _sw.Stop();\n");
                out.push_str(&format!("                long _durNs = {dur_ns};\n"));
                if timeout_enabled && !method.is_async {
                    // 同步超时：软超时（事后判定超时，无法中断阻塞代码）。
                    out.push_str(&format!("                if ({timeout_ms_expr} > 0 && _durNs / 1000000 > {timeout_ms_expr}) {{\n"));
                    out.push_str(&timeout_record);
                    out.push_str("                } else {\n");
                    out.push_str(&pass_record);
                    out.push_str("                }\n");
                } else {
                    out.push_str(&pass_record);
                }
            }
        };

        // 共享模板：对象构造 + Stopwatch + try/catch + 生命周期 + teardown。
        let emit_block = |out: &mut String, display: &str, kind: &str, args_str: &str| {
            out.push_str(&format!("            {cls} _obj = {new_expr};\n"));
            out.push_str("            Stopwatch _sw = Stopwatch.StartNew();\n");
            out.push_str("            try {\n");
            out.push_str("                if (_obj is IQIFSetup) { ((IQIFSetup)_obj).Setup(); }\n");
            emit_invoke(out, display, kind, args_str);
            out.push_str("            } catch (Exception ex) {\n");
            out.push_str("                _sw.Stop();\n");
            out.push_str(&format!("                long _durNs = {dur_ns};\n"));
            out.push_str(&format!("                if (ex.Message.StartsWith(\"QIF_SKIP:\")) {{ runner.{rs_method}(\"{display}\", QIFTestKind.{kind}, ex.Message{trait_param}); }} else {{ runner.{fail_method}(\"{display}\", QIFTestKind.{kind}, _durNs, ex.Message{fail_trait_param}); }}\n"));
            out.push_str("            }\n");
            if let Some(output_idx) = output_fixtures.get(cls.as_str()) {
                out.push_str(&format!("            this.runner.SetLastOutput(this._fixture_{cls}_{output_idx}.Output);\n"));
            }
            out.push_str("            try {\n");
            out.push_str(
                "                if (_obj is IQIFTeardown) { ((IQIFTeardown)_obj).Teardown(); }\n",
            );
            out.push_str("            } catch (Exception ignore) { }\n");
        };

        if method.attr_name == "Fact" || method.inline_data.is_empty() {
            out.push_str(&format!("            // [Fact] {display_name}\n"));
            if let Some(ref skip) = method.skip_reason {
                out.push_str(&format!("            this.runner.{rs_method}(\"{display_name}\", QIFTestKind.Fact, \"{skip}\"{trait_param});\n"));
            } else {
                emit_block(out, &display_name, "Fact", "");
            }
        } else {
            for data in &method.inline_data {
                let args: Vec<String> = data.iter().map(|n| n.to_string()).collect();
                let args_str = args.join(", ");
                let display_args: Vec<String> = data.iter().map(|n| n.display_fmt()).collect();
                let display_args_str = display_args.join(", ");
                let theory_display = format!("{display_name}({display_args_str})");

                out.push_str(&format!("            // [Theory] {theory_display}\n"));
                if let Some(ref skip) = method.skip_reason {
                    out.push_str(&format!("            this.runner.{rs_method}(\"{theory_display}\", QIFTestKind.Theory, \"{skip}\"{trait_param});\n"));
                } else {
                    emit_block(out, &theory_display, "Theory", &args_str);
                }
            }
        }
    };

    // --- 计算批次 ---
    let total = filtered.len();
    let num_batches = if total == 0 {
        0
    } else {
        total.div_ceil(BATCH_SIZE)
    };

    out.push_str("// RFC 032 Phase 4: auto-generated QIF test host (batched + parallel).\n");
    out.push_str("using Arc;\n");
    out.push_str("using Arc.QIF;\n");
    out.push_str("using Arc.Threading;\n");
    out.push_str("using Arc.Diagnostics;\n");
    out.push('\n');

    out.push_str("public class __QifTestHost {\n");
    out.push_str("    QIFRunner runner;\n");
    // Fixture instances as fields
    for (cls, params) in &ctor_map {
        for (i, p) in params.iter().enumerate() {
            let fixture_type = if p == "IQIFOutput" {
                "QIFOutputHelper"
            } else {
                p.as_str()
            };
            out.push_str(&format!("    {fixture_type} _fixture_{cls}_{i};\n"));
        }
    }
    out.push('\n');

    // Main: create instance and dispatch
    // 含 async Fact 时走 async Main → EventLoop；失败用 Environment.Exit（wrapper 固定 ret 0）
    if has_async {
        out.push_str("    public static async Task Main() {\n");
    } else {
        out.push_str("    public static int Main() {\n");
    }
    out.push_str("        __QifTestHost self = new __QifTestHost();\n");
    out.push_str("        QIFHost host = new QIFHost();\n");
    out.push_str("        self.runner = host.Runner;\n");
    out.push_str(&format!(
        "        self.runner.MaxParallel = {};\n",
        qif_opts.max_parallel
    ));
    out.push_str(&format!(
        "        self.runner.DefaultTimeoutMs = {};\n",
        qif_opts.default_timeout_ms
    ));
    out.push('\n');

    // Initialize fixtures on self
    for (cls, params) in &ctor_map {
        for (i, p) in params.iter().enumerate() {
            let fixture_type = if p == "IQIFOutput" {
                "QIFOutputHelper"
            } else {
                p.as_str()
            };
            out.push_str(&format!(
                "        self._fixture_{cls}_{i} = new {fixture_type}();\n"
            ));
        }
    }
    out.push('\n');

    // QIF_PROGRESS 诊断插桩：运行期环境变量开关的逐测试进度打印。
    // 烘焙进二进制但默认静默；设 QIF_PROGRESS=1 后每测试执行前打一行
    // `[qif-run] <idx> <fqn>`，用于定位静默执行模式下 0xC0000005 的崩溃点。
    let qif_names: Vec<String> = filtered
        .iter()
        .map(|m| {
            let fqn = if m.namespace.is_empty() {
                format!("{}.{}", m.class_name, m.method_name)
            } else {
                format!("{}.{}.{}", m.namespace, m.class_name, m.method_name)
            };
            format!("\"{}\"", escape_arc_string(&fqn))
        })
        .collect();
    out.push_str(&format!(
        "        string[] __qif_names = [ {} ];\n",
        qif_names.join(", ")
    ));
    out.push_str(
        "        string __qif_progress = Environment.GetEnvironmentVariable(\"QIF_PROGRESS\");\n",
    );
    out.push('\n');

    if total == 0 {
        if has_async {
            out.push_str("        return;\n");
        } else {
            out.push_str("        return 0;\n");
        }
        out.push_str("    }\n");
        out.push_str("}\n");
        return out;
    }

    // Sequential or parallel dispatch - call instance methods on self.
    // async Fact 强制串行：Parallel.For 回调无法 await。
    let use_parallel = qif_opts.parallel && total > 1 && !has_async;
    if use_parallel {
        out.push_str("        // Parallel execution (XUnit default)\n");
        out.push_str("        ParallelOptions parOpts = new ParallelOptions();\n");
        out.push_str("        // 绑定默认线程池：Scheduler 字段被 codegen 读取为 rt_parallel_for 的 pool 实参，\n");
        out.push_str("        // 缺省为 null 时 rt_parallel_for 退化为同步执行（无并行加速）。\n");
        out.push_str("        parOpts.Scheduler = new ThreadPoolScheduler();\n");
        out.push_str("        parOpts.MaxDegreeOfParallelism = self.runner.MaxParallel;\n");
        out.push_str(&format!(
            "        Parallel.For(0, {total}, parOpts, idx => self.DispatchTest(idx));\n"
        ));
    } else {
        if has_async && qif_opts.parallel {
            out.push_str(
                "        // Sequential: async Fact/Theory 禁用 Parallel.For（无法 await）\n",
            );
        } else {
            out.push_str("        // Sequential execution\n");
        }
        out.push_str(&format!(
            "        for (int idx = 0; idx < {total}; idx = idx + 1) {{\n"
        ));
        out.push_str("            if (__qif_progress != \"\") { Console.WriteLine(\"[qif-run] \" + Convert.ToString(idx) + \" \" + __qif_names[idx]); }\n");
        for b in 0..num_batches {
            let start = b * BATCH_SIZE;
            let end = std::cmp::min(start + BATCH_SIZE, total);
            let await_kw = if has_async { "await " } else { "" };
            if b == 0 {
                out.push_str(&format!(
                    "            if (idx < {end}) {{ {await_kw}self.RunBatch{b}(idx); }}\n"
                ));
            } else if b < num_batches - 1 {
                out.push_str(&format!("            else if (idx < {end}) {{ {await_kw}self.RunBatch{b}(idx - {start}); }}\n"));
            } else {
                out.push_str(&format!(
                    "            else {{ {await_kw}self.RunBatch{b}(idx - {start}); }}\n"
                ));
            }
        }
        out.push_str("        }\n");
    }
    out.push('\n');

    // Report
    let fmt = qif_opts.output_format.as_str();
    if fmt == "json" {
        out.push_str("        QIFReporting.WriteJsonReport(self.runner);\n");
    } else if fmt == "junit" {
        out.push_str("        QIFReporting.WriteJUnitXml(self.runner);\n");
    } else {
        out.push_str("        QIFReporting.WriteReport(self.runner);\n");
    }
    // RFC 032 §7：报告产物落盘（report.json / .arcqif），在 Environment.Exit 前执行。
    // output_dir 已由 CLI 解析为绝对路径并转义嵌入；布尔开关为编译期常量。
    out.push_str(&format!(
        "        QIFReporting.PersistArtifacts(self.runner, \"{}\", {}, {});\n",
        escape_arc_string(&qif_opts.output_dir),
        qif_opts.emit_json_report,
        qif_opts.persist_results,
    ));
    // H1: 报告后 Environment.Exit→_exit，跳过 Main 返回后的局部/CRT 析构 free
    // 风暴（应力：Wiki/Summary 已打完仍 0xC0000005）。
    out.push_str("        if (self.runner.HasFailures) { Environment.Exit(1); }\n");
    out.push_str("        Environment.Exit(0);\n");
    out.push_str("    }\n");
    out.push('\n');

    // DispatchTest: sync only（并行路径）；async 套件不生成
    if use_parallel {
        out.push_str("    void DispatchTest(int index) {\n");
        for b in 0..num_batches {
            let start = b * BATCH_SIZE;
            let end = std::cmp::min(start + BATCH_SIZE, total);
            if b == 0 {
                out.push_str(&format!(
                    "        if (index < {end}) {{ this.RunBatch{b}(index); return; }}\n"
                ));
            } else if b < num_batches - 1 {
                out.push_str(&format!(
                    "        if (index < {end}) {{ this.RunBatch{b}(index - {start}); return; }}\n"
                ));
            } else {
                out.push_str(&format!("        this.RunBatch{b}(index - {start});\n"));
            }
        }
        out.push_str("    }\n");
        out.push('\n');
    }

    // Generate RunBatch{N} functions
    let batch_sig = if has_async { "async Task" } else { "void" };
    for b in 0..num_batches {
        let start = b * BATCH_SIZE;
        let end = std::cmp::min(start + BATCH_SIZE, total);
        out.push_str(&format!("    {batch_sig} RunBatch{b}(int idx) {{\n"));
        out.push_str("        if (idx == 0) {\n");
        emit_test_call(&mut out, filtered[start], start, 0);
        out.push_str("            return;\n");
        out.push_str("        }\n");
        for (i, m) in filtered.iter().enumerate().take(end).skip(start + 1) {
            out.push_str(&format!("        if (idx == {}) {{\n", i - start));
            emit_test_call(&mut out, m, i, 0);
            out.push_str("            return;\n");
            out.push_str("        }\n");
        }
        out.push_str("    }\n");
        out.push('\n');
    }

    out.push_str("}\n");
    out
}

// ────────────────────────────────────────────────────────────────────
// QIF-6 XUnit 风格过滤表达式引擎
// ────────────────────────────────────────────────────────────────────
// 语法（对标 xUnit v2 filter）：
//   expr       := or_expr
//   or_expr    := and_expr ('|' and_expr)*
//   and_expr   := atom ('&' atom)*
//   atom       := '!' atom | comp | '(' or_expr ')'
//   comp       := field '~' value        // contains
//              | field '~!' value        // not contains
//              | field '=' value         // exact
//              | field '!' value         // not equal
//              | value                   // 简写：等同 FullyQualifiedName~value
//   field      := FullyQualifiedName | ClassName | MethodName | Name | Trait | Kind | Collection
//
// 语义：
//   - 顶层 `,` 作为 OR（xUnit CLI 行为），同 `|`。
//   - 未指定字段的 `value` 默认匹配 FullyQualifiedName（兼容老 `--filter substring`）。
//   - Trait 字段特殊：value 格式 `k=v` 或仅 `k`，在 traits 列表里匹配任意一项。
//   - Kind 字段值：Fact | Theory | Integration | E2e | Benchmark | Property | Snapshot | Contract
// ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub(crate) enum QifFilterExpr {
    /// 叶子：字段匹配
    Compare {
        field: String,
        op: QifFilterOp,
        value: String,
    },
    /// 逻辑或
    Or(Vec<QifFilterExpr>),
    /// 逻辑与
    And(Vec<QifFilterExpr>),
    /// 逻辑非
    Not(Box<QifFilterExpr>),
}

#[derive(Clone, Debug)]
pub(crate) enum QifFilterOp {
    Contains,
    NotContains,
    Eq,
    NotEq,
}

impl QifFilterExpr {
    /// 解析过滤字符串；空字符串返回 `None`（表示「全选」）。
    /// 错误返回带上下文的 `String`。
    pub(crate) fn parse(input: &str) -> Result<Option<Self>, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        // 顶层：支持 `,` 作为 OR（与 `|` 等价；xUnit CLI 习惯）。
        let or_parts = split_top_level(trimmed, ',')
            .into_iter()
            .map(|s| Self::parse_or(&s))
            .collect::<Result<Vec<_>, _>>()?;
        if or_parts.is_empty() {
            return Ok(None);
        }
        if or_parts.len() == 1 {
            Ok(Some(or_parts.into_iter().next().unwrap()))
        } else {
            Ok(Some(QifFilterExpr::Or(or_parts)))
        }
    }

    fn parse_or(input: &str) -> Result<Self, String> {
        let parts = split_top_level(input, '|')
            .into_iter()
            .map(|s| Self::parse_and(&s))
            .collect::<Result<Vec<_>, _>>()?;
        if parts.len() == 1 {
            Ok(parts.into_iter().next().unwrap())
        } else {
            Ok(QifFilterExpr::Or(parts))
        }
    }

    fn parse_and(input: &str) -> Result<Self, String> {
        let parts = split_top_level(input, '&')
            .into_iter()
            .map(|s| Self::parse_atom(&s))
            .collect::<Result<Vec<_>, _>>()?;
        if parts.len() == 1 {
            Ok(parts.into_iter().next().unwrap())
        } else {
            Ok(QifFilterExpr::And(parts))
        }
    }

    fn parse_atom(input: &str) -> Result<Self, String> {
        let s = input.trim();
        if s.is_empty() {
            return Err("empty atom in filter expression".to_string());
        }
        // 括号子表达式
        if s.starts_with('(') && s.ends_with(')') {
            let inner = &s[1..s.len() - 1];
            // 验证括号配对是否已在 split_top_level 处理
            return Self::parse_or(inner);
        }
        // 逻辑非
        if let Some(rest) = s.strip_prefix('!') {
            let rest = rest.trim_start();
            if rest.is_empty() {
                return Err("'!' requires an operand".to_string());
            }
            let inner = Self::parse_atom(rest)?;
            return Ok(QifFilterExpr::Not(Box::new(inner)));
        }
        // 字段 操作符 值
        if let Some((field, op, value)) = parse_compare(s)? {
            Ok(QifFilterExpr::Compare { field, op, value })
        } else {
            // 简写：视为 FullyQualifiedName ~ value
            Ok(QifFilterExpr::Compare {
                field: "FullyQualifiedName".to_string(),
                op: QifFilterOp::Contains,
                value: s.to_string(),
            })
        }
    }

    /// 在给定方法上求值。`get_field` 由调用方提供以避免方法签名膨胀。
    pub(crate) fn matches<F>(&self, get_field: &F) -> bool
    where
        F: Fn(&str) -> Vec<String>,
    {
        match self {
            QifFilterExpr::Compare { field, op, value } => {
                let haystacks = get_field(field);
                if haystacks.is_empty() {
                    return false;
                }
                match op {
                    QifFilterOp::Contains => haystacks.iter().any(|h| h.contains(value.as_str())),
                    QifFilterOp::NotContains => {
                        !haystacks.iter().any(|h| h.contains(value.as_str()))
                    }
                    QifFilterOp::Eq => haystacks.iter().any(|h| h == value.as_str()),
                    QifFilterOp::NotEq => !haystacks.iter().any(|h| h == value.as_str()),
                }
            }
            QifFilterExpr::Or(parts) => parts.iter().any(|p| p.matches(get_field)),
            QifFilterExpr::And(parts) => parts.iter().all(|p| p.matches(get_field)),
            QifFilterExpr::Not(inner) => !inner.matches(get_field),
        }
    }
}

/// 解析 `field op value`；返回 `(field, op, value)`。
/// 支持的操作符（含最长匹配优先）：`~!` `!=` `~` `=`。
fn parse_compare(s: &str) -> Result<Option<(String, QifFilterOp, String)>, String> {
    // 最长两字符优先
    for (op_str, op) in [
        ("~!", QifFilterOp::NotContains),
        ("!=", QifFilterOp::NotEq),
        ("~", QifFilterOp::Contains),
        ("=", QifFilterOp::Eq),
    ] {
        if let Some(idx) = s.find(op_str) {
            let field = s[..idx].trim().to_string();
            let value = s[idx + op_str.len()..].trim().to_string();
            if field.is_empty() {
                return Err(format!("empty field before '{op_str}' in '{s}'"));
            }
            if value.is_empty() {
                return Err(format!("empty value after '{op_str}' in '{s}'"));
            }
            return Ok(Some((field, op, value)));
        }
    }
    Ok(None)
}

/// 按顶层分隔符切分（忽略括号内的分隔符）。
fn split_top_level(input: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in input.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(input[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(input[start..].to_string());
    parts
}

/// 将过滤表达式 + 命名空间 + Kind 应用于方法列表，返回已过滤的方法集。
/// 三者以 AND 组合；filter 使用 QIF-6 XUnit 风格表达式引擎，namespace 为
/// 命名空间前缀匹配（ClassName 必须以 `<ns>.` 开头或等于 `<ns>`），kind 为
/// 精确匹配 `attr_name`（Fact/Theory/...）。
pub(crate) fn apply_qif_filter(
    methods: Vec<QifTestMethod>,
    filter: &str,
    namespace: &str,
    kind: &str,
) -> Result<Vec<QifTestMethod>, String> {
    let expr = QifFilterExpr::parse(filter)?;
    let out = methods
        .into_iter()
        .filter(|m| {
            // namespace: 命名空间以 `<ns>` 开头或等于 `<ns>`
            if !namespace.is_empty() {
                let ns = namespace.trim();
                let mns = m.namespace.as_str();
                if !(mns == ns || mns.starts_with(&format!("{ns}."))) {
                    return false;
                }
            }
            // kind: 精确匹配 attr_name
            if !kind.is_empty() {
                let k = kind.trim();
                if m.attr_name != k {
                    return false;
                }
            }
            // filter: XUnit 表达式引擎
            if let Some(ref e) = expr {
                return e.matches(&|field: &str| -> Vec<String> { field_values(m, field) });
            }
            true
        })
        .collect();
    Ok(out)
}

/// 字段值提取：供过滤引擎的 `get_field` 回调使用。
/// 一个字段可能对应多个值（如 Trait 多键值、FullyQualifiedName 同时含类名与显示名）。
pub(crate) fn field_values(m: &QifTestMethod, field: &str) -> Vec<String> {
    match field {
        "FullyQualifiedName" | "Name" => {
            let fqn = if m.namespace.is_empty() {
                format!("{}.{}", m.class_name, m.method_name)
            } else {
                format!("{}.{}.{}", m.namespace, m.class_name, m.method_name)
            };
            let display = if m.display_name.is_empty() {
                fqn.clone()
            } else {
                m.display_name.clone()
            };
            vec![display, fqn, m.class_name.clone(), m.method_name.clone()]
        }
        "ClassName" => vec![m.class_name.clone()],
        "MethodName" => vec![m.method_name.clone()],
        "Namespace" => vec![m.namespace.clone()],
        "Trait" => m.traits.iter().map(|(k, v)| format!("{k}:{v}")).collect(),
        "Kind" => vec![m.attr_name.clone()],
        "Collection" => m
            .collection_name
            .as_ref()
            .map(|c| vec![c.clone()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// 计算方法显示名（统一供 list / filter 使用）。
fn display_of(m: &QifTestMethod) -> String {
    if m.display_name.is_empty() {
        format!("{}.{}", m.class_name, m.method_name)
    } else {
        m.display_name.clone()
    }
}

/// --list-tests --list-format json 的 JSON 输出（CI 友好）。
fn print_list_json(methods: &[QifTestMethod]) {
    // 稳定排序
    let mut sorted: Vec<&QifTestMethod> = methods.iter().collect();
    sorted.sort_by_key(|a| display_of(a));
    println!("{{");
    println!("  \"total\": {},", sorted.len());
    println!("  \"tests\": [");
    for (i, m) in sorted.iter().enumerate() {
        let display = display_of(m);
        let kind = m.attr_name.as_str();
        let skip = m.skip_reason.as_deref().unwrap_or("");
        let coll = m.collection_name.as_deref().unwrap_or("");
        let mut traits_str = String::from("[");
        for (j, (k, v)) in m.traits.iter().enumerate() {
            if j > 0 {
                traits_str.push_str(", ");
            }
            traits_str.push_str(&format!("\"{k}={v}\""));
        }
        traits_str.push(']');
        let term = if i == sorted.len() - 1 { "" } else { "," };
        println!(
            "    {{\"name\": \"{}\", \"kind\": \"{}\", \"order\": {}, \"skip\": \"{}\", \"collection\": \"{}\", \"traits\": {}}}{}",
            display.replace('\\', "\\\\").replace('"', "\\\""),
            kind,
            m.order,
            skip.replace('\\', "\\\\").replace('"', "\\\""),
            coll.replace('\\', "\\\\").replace('"', "\\\""),
            traits_str,
            term,
        );
    }
    println!("  ]");
    println!("}}");
}

/// RFC 038: AST 遍历收集 `[AITool]` 标记的工具方法，并做签名校验。
///
/// 校验（编译期报清晰错误，非法签名不静默通过）：
/// - 工具名唯一（`AIToolSet.Add` 同名会静默覆盖，编译期即拦截）
/// - 方法非 static / 非泛型
/// - 参数仅 `string/int/long/double/bool/string[]`（无 ref/out/in）
/// - 返回仅 `void/string/Task<string>/Task<void>`
/// - 工具类构造仅 `()` 或唯一 `ctor(IServiceProvider)`（DI 桥）
fn collect_ai_tool_methods(program: &ast::Program) -> Result<Vec<AIToolMethod>, String> {
    // 全局类索引（完整路径 → ClassDef），供模型参数解析公开字段 + [Description]。
    let mut class_index: HashMap<String, &ast::ClassDef> = HashMap::new();
    index_ai_classes(&program.items, "", &mut class_index);
    let mut methods = Vec::new();
    collect_ai_from_items(&program.items, "", &class_index, &mut methods)?;
    let mut seen: HashMap<String, String> = HashMap::new();
    for m in &methods {
        if let Some(prev) = seen.get(&m.tool_name) {
            return Err(format!(
                "duplicate [AITool] name '{}' declared on {}.{} and {}",
                m.tool_name, prev, m.class_path, m.method_name
            ));
        }
        seen.insert(
            m.tool_name.clone(),
            format!("{}.{}", m.class_path, m.method_name),
        );
    }
    Ok(methods)
}

fn collect_ai_from_items(
    items: &[ast::Spanned<ast::Item>],
    ns_prefix: &str,
    class_index: &HashMap<String, &ast::ClassDef>,
    out: &mut Vec<AIToolMethod>,
) -> Result<(), String> {
    for spanned in items {
        match &spanned.node {
            ast::Item::Class(c) => {
                collect_ai_from_class(c, ns_prefix, class_index, out)?;
            }
            ast::Item::Namespace(ns) => {
                let joined: Vec<String> = ns.path.iter().map(|i| i.to_string()).collect();
                let segs = joined.join(".");
                let child_prefix = if ns_prefix.is_empty() {
                    segs
                } else if segs.is_empty() {
                    ns_prefix.to_string()
                } else {
                    format!("{ns_prefix}.{segs}")
                };
                collect_ai_from_items(&ns.items, &child_prefix, class_index, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// 递归索引全部类（完整路径 → &ClassDef），供模型参数解析。
fn index_ai_classes<'a>(
    items: &'a [ast::Spanned<ast::Item>],
    ns_prefix: &str,
    index: &mut HashMap<String, &'a ast::ClassDef>,
) {
    for spanned in items {
        match &spanned.node {
            ast::Item::Class(c) => {
                let path = if ns_prefix.is_empty() {
                    c.name.to_string()
                } else {
                    format!("{ns_prefix}.{}", c.name)
                };
                index.insert(path, c);
            }
            ast::Item::Namespace(ns) => {
                let joined: Vec<String> = ns.path.iter().map(|i| i.to_string()).collect();
                let segs = joined.join(".");
                let child_prefix = if ns_prefix.is_empty() {
                    segs
                } else if segs.is_empty() {
                    ns_prefix.to_string()
                } else {
                    format!("{ns_prefix}.{segs}")
                };
                index_ai_classes(&ns.items, &child_prefix, index);
            }
            _ => {}
        }
    }
}

fn collect_ai_from_class(
    c: &ast::ClassDef,
    ns_prefix: &str,
    class_index: &HashMap<String, &ast::ClassDef>,
    out: &mut Vec<AIToolMethod>,
) -> Result<(), String> {
    let class_name = c.name.to_string();
    let class_path = if ns_prefix.is_empty() {
        class_name.clone()
    } else {
        format!("{ns_prefix}.{class_name}")
    };
    if !c.generics.is_empty() {
        let has_tool = c.methods.iter().any(|m| {
            m.node
                .sig
                .attributes
                .iter()
                .any(|a| a.path.first().map(|i| i.to_string()).unwrap_or_default() == "AITool")
        });
        if has_tool {
            return Err(format!("[AITool] class {class_path} must not be generic"));
        }
    }
    for method in &c.methods {
        let sig = &method.node.sig;
        let attr = sig
            .attributes
            .iter()
            .find(|a| a.path.first().map(|i| i.to_string()).unwrap_or_default() == "AITool");
        let Some(attr) = attr else { continue };
        if sig.modifier == ast::MethodModifier::Static {
            return Err(format!(
                "[AITool] method {class_path}.{} must not be static",
                sig.name
            ));
        }
        if !sig.generics.is_empty() {
            return Err(format!(
                "[AITool] method {class_path}.{} must not be generic",
                sig.name
            ));
        }
        let ctor_kind = ai_tool_ctor_kind(c)?;
        let (tool_name, capability, require_approval) = ai_tool_attr_meta(attr, sig.name.as_ref());
        // 工具描述由方法级 [Description] 提供（已从 AITool 属性移除）。
        let description = ai_description(&sig.attributes);
        let mut params: Vec<AIToolParam> = Vec::new();
        for p in &sig.params {
            if p.is_ref || p.is_out || p.is_in {
                return Err(format!(
                    "[AITool] method {class_path}.{} param '{}' must not be ref/out/in",
                    sig.name, p.name
                ));
            }
            params.push(ai_tool_param(
                p,
                ns_prefix,
                class_index,
                &class_path,
                sig.name.as_ref(),
            )?);
        }
        let ret = ai_tool_ret_kind(&sig.ret, &class_path, sig.name.as_ref())?;
        out.push(AIToolMethod {
            class_path: class_path.clone(),
            method_name: sig.name.to_string(),
            tool_name,
            description,
            capability,
            require_approval,
            params,
            ret,
            ctor_kind,
        });
    }
    Ok(())
}

/// 工具类构造规则：无构造 → noarg；唯一构造 `ctor(IServiceProvider)` → provider；其余拒绝。
fn ai_tool_ctor_kind(c: &ast::ClassDef) -> Result<String, String> {
    match c.constructors.len() {
        0 => Ok("noarg".to_string()),
        1 => {
            let ctor = &c.constructors[0].node;
            let ok = ctor.params.len() == 1
                && matches!(&ctor.params[0].ty.node, ast::Type::Named { path, generics }
                    if generics.is_empty()
                        && path.last().map(|i| i.to_string()).unwrap_or_default() == "IServiceProvider");
            if ok {
                Ok("provider".to_string())
            } else {
                Err(format!(
                    "class {} has [AITool] methods: constructor must be () or (IServiceProvider) for DI bridge",
                    c.name
                ))
            }
        }
        _ => Err(format!(
            "class {} has [AITool] methods: multiple constructors not supported; use () or (IServiceProvider)",
            c.name
        )),
    }
}

/// `[AITool]` 参数绑定：标量（string/int/long/double/bool/string[]）或模型类。
///
/// 模型参数（如 `[Description] UserModel model`）：整段参数 JSON 反序列化为模型实例，
/// schema 与绑定依据模型公开字段/属性的 `[Description]` 生成（`[AITool]` + `[Description]` 组合）。
fn ai_tool_param(
    p: &ast::Param,
    ns_prefix: &str,
    class_index: &HashMap<String, &ast::ClassDef>,
    class_path: &str,
    method_name: &str,
) -> Result<AIToolParam, String> {
    let err = || {
        format!(
            "[AITool] method {class_path}.{method_name} param '{}': unsupported type; allowed: string / int / long / double / bool / string[] or a [Description]-annotated model class",
            p.name
        )
    };
    let desc = ai_description(&p.attributes);
    let ty = &p.ty.node;
    // 扁平基元参数。
    if let ast::Type::Named { path, generics } = ty {
        if generics.is_empty() {
            let base = path.last().map(|i| i.to_string()).unwrap_or_default();
            if matches!(base.as_str(), "string" | "int" | "long" | "double" | "bool") {
                return Ok(AIToolParam {
                    name: p.name.to_string(),
                    ty: base,
                    description: desc,
                    model_fields: vec![],
                });
            }
        }
    }
    if let ast::Type::Array { inner } = ty {
        let is_string_arr = matches!(&inner.node, ast::Type::Named { path, generics }
            if generics.is_empty() && path.last().map(|i| i.to_string()).unwrap_or_default() == "string");
        if is_string_arr {
            return Ok(AIToolParam {
                name: p.name.to_string(),
                ty: "string[]".to_string(),
                description: desc,
                model_fields: vec![],
            });
        }
    }
    // 模型参数：解析类 + 提取 `[Description]` 字段。
    if let ast::Type::Named { path, generics } = ty {
        if generics.is_empty() {
            let rel: Vec<String> = path.iter().map(|i| i.to_string()).collect();
            let rel = rel.join(".");
            if let Some(model_path) = resolve_model_path(&rel, ns_prefix, class_index) {
                let fields = ai_model_fields(&model_path, class_index, class_path, method_name)?;
                return Ok(AIToolParam {
                    name: p.name.to_string(),
                    ty: model_path,
                    description: desc,
                    model_fields: fields,
                });
            }
        }
    }
    Err(err())
}

/// 解析模型类完整路径：优先当前命名空间相对解析，其次全局/绝对。
fn resolve_model_path(
    rel: &str,
    ns_prefix: &str,
    class_index: &HashMap<String, &ast::ClassDef>,
) -> Option<String> {
    if !ns_prefix.is_empty() {
        let cand = format!("{ns_prefix}.{rel}");
        if class_index.contains_key(&cand) {
            return Some(cand);
        }
    }
    if class_index.contains_key(rel) {
        return Some(rel.to_string());
    }
    None
}

/// 提取模型类可绑定字段：公开实例字段 / 公开 set 属性（标量类型），含 `[Description]`。
fn ai_model_fields(
    model_path: &str,
    class_index: &HashMap<String, &ast::ClassDef>,
    class_path: &str,
    method_name: &str,
) -> Result<Vec<AIToolModelField>, String> {
    let class = class_index.get(model_path).ok_or_else(|| {
        format!("[AITool] method {class_path}.{method_name}: model class {model_path} not found")
    })?;
    let mut fields = Vec::new();
    for f in &class.fields {
        if f.vis != ast::Visibility::Public || f.is_static {
            continue;
        }
        let Some(base) = ai_scalar_base(&f.ty.node) else {
            continue;
        };
        fields.push(AIToolModelField {
            name: f.name.to_string(),
            ty: base,
            description: ai_description(&f.attributes),
        });
    }
    for p in &class.properties {
        if p.vis != ast::Visibility::Public
            || !p.has_set
            || p.modifier == ast::MethodModifier::Static
        {
            continue;
        }
        let Some(base) = ai_scalar_base(&p.ty.node) else {
            continue;
        };
        fields.push(AIToolModelField {
            name: p.name.to_string(),
            ty: base,
            description: ai_description(&p.attributes),
        });
    }
    Ok(fields)
}

/// 返回标量类型名（string/int/long/double/bool），非标量返回 None。
fn ai_scalar_base(ty: &ast::Type) -> Option<String> {
    if let ast::Type::Named { path, generics } = ty {
        if generics.is_empty() {
            let base = path.last().map(|i| i.to_string()).unwrap_or_default();
            if matches!(base.as_str(), "string" | "int" | "long" | "double" | "bool") {
                return Some(base);
            }
        }
    }
    None
}

/// 读取 `[Description("...")]` 的首个字符串参数（方法/参数/字段通用）。
fn ai_description(attrs: &[ast::Attribute]) -> String {
    for a in attrs {
        if a.path.first().map(|i| i.to_string()).unwrap_or_default() == "Description" {
            for arg in &a.args {
                if let ast::AttributeArg::String(s) = arg {
                    return s.clone();
                }
            }
        }
    }
    String::new()
}

fn ai_tool_ret_kind(
    ret: &Option<ast::Spanned<ast::Type>>,
    class_path: &str,
    method_name: &str,
) -> Result<String, String> {
    let err = || {
        format!(
            "[AITool] method {class_path}.{method_name}: unsupported return type; allowed: string / void / Task<string> / Task<void>"
        )
    };
    let Some(rt) = ret else {
        return Ok("void".to_string());
    };
    match &rt.node {
        ast::Type::Named { path, generics } if generics.is_empty() => {
            let base = path.last().map(|i| i.to_string()).unwrap_or_default();
            match base.as_str() {
                "void" | "string" => Ok(base),
                _ => Err(err()),
            }
        }
        ast::Type::Named { path, generics }
            if path.last().map(|i| i.to_string()).unwrap_or_default() == "Task" =>
        {
            if generics.len() != 1 {
                return Err(err());
            }
            match &generics[0].node {
                ast::Type::Named { path, generics }
                    if generics.is_empty()
                        && matches!(
                            path.last()
                                .map(|i| i.to_string())
                                .unwrap_or_default()
                                .as_str(),
                            "string" | "void"
                        ) =>
                {
                    Ok(format!(
                        "Task<{}>",
                        path.last().map(|i| i.to_string()).unwrap_or_default()
                    ))
                }
                _ => Err(err()),
            }
        }
        _ => Err(err()),
    }
}

/// 从 `[AITool]` 属性读取 Name/Capability/RequireApproval（位置参数 0 或命名 Name）。
/// 工具描述不在此处——由方法级 `[Description]` 属性独立提供（`[AITool]` + `[Description]` 组合）。
fn ai_tool_attr_meta(attr: &ast::Attribute, method_name: &str) -> (String, String, bool) {
    let mut name = String::new();
    let mut capability = "ai.Tool".to_string();
    let mut require_approval = false;
    for arg in &attr.args {
        match arg {
            ast::AttributeArg::String(s) if name.is_empty() => name = s.clone(),
            ast::AttributeArg::Named { name: n, value } => match n.as_str() {
                "Name" => {
                    if let ast::AttributeArg::String(s) = value.as_ref() {
                        name = s.clone();
                    }
                }
                "Capability" => {
                    if let ast::AttributeArg::String(s) = value.as_ref() {
                        capability = s.clone();
                    }
                }
                "RequireApproval" => {
                    if let ast::AttributeArg::Bool(b) = value.as_ref() {
                        require_approval = *b;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    if name.is_empty() {
        name = method_name.to_string();
    }
    (name, capability, require_approval)
}

/// RFC 038: 生成合成 `__AIToolHost`（程序特定工具宿主；普通 build / test / publish 均生效）。
///
/// 此处合成的是**程序特定宿主** `__AIToolHost`（内部类，用户无感；显式静态注册，运行期零反射）：
/// - `__AIToolHost.Create(IServiceProvider)`：构造 `AIToolSet`，每工具一个
///   `AIToolDescriptor`（含编译期 `ParametersSchema`）+ 包装 handler
/// - 每工具一个 `__AITool_<Class>_<Method> : AIToolHandler`：覆写
///   `Name/Capability/InvokeAsync`；`InvokeAsync` 内 `AIToolArgsReader` 按参数名
///   取扁平基元值 → await/直调用户方法 → `AIToolResult.Ok/Fail`（异常捕获）；
///   同步方法在生成侧以非 async 直调适配（契约唯一正道 InvokeAsync 不变）。
/// - 非静态工具类实例化：先经 `IServiceProvider` 解析（DI 优先），未注册则回退
///   参数构造（`()` 或 `(IServiceProvider)`）——透明支持复杂业务注入。
fn generate_ai_tool_host(methods: &[AIToolMethod]) -> String {
    let mut out = String::new();
    out.push_str(
        "// RFC 038 / RFC 016 M6 / RFC 012 S5: auto-generated declarative AI tool host.\n",
    );
    out.push_str(
        "// __AIToolHost 为 internal（用户无感）；其 __RegisterGlobal 在程序入口被调用，\n",
    );
    out.push_str("// 注册为 AIHost 的默认工具源，实例化 AIHost 即自动获得全部 [AITool] 工具。\n");
    out.push_str("using Arc;\n");
    out.push_str("using Arc.Agent.Messages;\n");
    out.push_str("using Arc.Agent.Tools;\n");
    out.push_str("using Arc.Agent.Sessions;\n");
    out.push('\n');
    out.push_str("internal class __AIToolHost {\n");
    out.push_str(
        "    // RFC 016 M6：程序入口（Main 首行注入本调用）后，把编译期合成的工具集注册为\n",
    );
    out.push_str(
        "    // AIHost 的默认工具源——实例化 AIHost 即自动获得全部 [AITool] 工具，用户零显式\n",
    );
    out.push_str("    // 装配；真实生效仍由 AICapabilitySet 白名单 fail-closed 授权。\n");
    out.push_str("    public static void __RegisterGlobal() {\n");
    out.push_str("        AIHost.SetDefaultToolSource((services: IServiceProvider) => __AIToolHost.Create(services));\n");
    out.push_str("    }\n");
    out.push('\n');
    out.push_str("    public static AIToolSet Create(IServiceProvider services) {\n");
    out.push_str("        AIToolSet set = new AIToolSet();\n");
    let wrapper_names = ai_tool_wrapper_names(methods);
    for (i, m) in methods.iter().enumerate() {
        let var = format!("d{i}");
        let wrapper = &wrapper_names[i];
        out.push_str(&format!(
            "        AIToolDescriptor {var} = new AIToolDescriptor(\"{}\", \"{}\", \"{}\", {});\n",
            ai_esc(&m.tool_name),
            ai_esc(&m.description),
            ai_esc(&m.capability),
            m.require_approval
        ));
        out.push_str(&format!(
            "        {var}.ParametersSchema = \"{}\";\n",
            ai_esc(&ai_parameters_schema(m))
        ));
        out.push_str(&format!(
            "        set.Add({var}, new {wrapper}(services));\n"
        ));
    }
    out.push_str("        return set;\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    for (i, m) in methods.iter().enumerate() {
        let wrapper = &wrapper_names[i];
        out.push_str(&format!("\ninternal class {wrapper} : AIToolHandler {{\n"));
        out.push_str(&format!("    private {} _tool;\n", m.class_path));
        out.push_str(&format!(
            "    public {wrapper}(IServiceProvider services) {{\n"
        ));
        // DI 优先：工具类已注册到 IServiceProvider → 由容器解析（支持复杂业务注入）。
        out.push_str("        if (services != null) {\n");
        out.push_str(&format!(
            "            object? svc = services.GetService(typeof({0}));\n",
            m.class_path
        ));
        out.push_str(&format!(
            "            if (svc != null) {{ _tool = ({0})svc; return; }}\n",
            m.class_path
        ));
        out.push_str("        }\n");
        if m.ctor_kind == "provider" {
            out.push_str(&format!(
                "        _tool = new {}(services);\n",
                m.class_path
            ));
        } else {
            out.push_str(&format!("        _tool = new {}();\n", m.class_path));
        }
        out.push_str("    }\n");
        out.push_str(&format!(
            "    public override string Name {{ get {{ return \"{}\"; }} }}\n",
            ai_esc(&m.tool_name)
        ));
        out.push_str(&format!(
            "    public override string Capability {{ get {{ return \"{}\"; }} }}\n",
            ai_esc(&m.capability)
        ));
        out.push_str("    public override async Task<AIToolResult> InvokeAsync(AIToolCall call, CancellationToken cancellationToken) {\n");
        out.push_str(
            "        string cid = call != null && call.CallId != null ? call.CallId : \"\";\n",
        );
        out.push_str("        try {\n");
        out.push_str("            AIToolArgsReader args = new AIToolArgsReader(call != null && call.ArgumentsJson != null ? call.ArgumentsJson : \"\");\n");
        // 绑定：标量参数内联取值；模型参数先构造实例 + 逐字段反序列化，再作为实参。
        let mut prelude: Vec<String> = Vec::new();
        let mut call_args: Vec<String> = Vec::new();
        for (i, p) in m.params.iter().enumerate() {
            if p.model_fields.is_empty() {
                call_args.push(match p.ty.as_str() {
                    "string" => format!("args.GetString(\"{}\")", p.name),
                    "int" => format!("args.GetInt(\"{}\")", p.name),
                    "long" => format!("args.GetLong(\"{}\")", p.name),
                    "double" => format!("args.GetDouble(\"{}\")", p.name),
                    "bool" => format!("args.GetBool(\"{}\")", p.name),
                    "string[]" => format!("args.GetStringArray(\"{}\")", p.name),
                    _ => unreachable!("validated param type"),
                });
            } else {
                let var = format!("m{i}");
                prelude.push(format!("            {0} {var} = new {0}();", p.ty));
                for f in &p.model_fields {
                    let getter = match f.ty.as_str() {
                        "string" => format!("args.GetString(\"{}\")", f.name),
                        "int" => format!("args.GetInt(\"{}\")", f.name),
                        "long" => format!("args.GetLong(\"{}\")", f.name),
                        "double" => format!("args.GetDouble(\"{}\")", f.name),
                        "bool" => format!("args.GetBool(\"{}\")", f.name),
                        _ => unreachable!("validated model field type"),
                    };
                    prelude.push(format!("            {var}.{0} = {getter};", f.name));
                }
                call_args.push(var);
            }
        }
        for line in &prelude {
            out.push_str(line);
            out.push('\n');
        }
        let call_expr = format!("_tool.{}({})", m.method_name, call_args.join(", "));
        match m.ret.as_str() {
            "void" => {
                out.push_str(&format!("            {call_expr};\n"));
                out.push_str("            return AIToolResult.Ok(cid, \"\");\n");
            }
            "string" => {
                out.push_str(&format!("            string result = {call_expr};\n"));
                out.push_str(
                    "            return AIToolResult.Ok(cid, result != null ? result : \"\");\n",
                );
            }
            "Task<string>" => {
                out.push_str(&format!("            string result = await {call_expr};\n"));
                out.push_str(
                    "            return AIToolResult.Ok(cid, result != null ? result : \"\");\n",
                );
            }
            "Task<void>" => {
                out.push_str(&format!("            await {call_expr};\n"));
                out.push_str("            return AIToolResult.Ok(cid, \"\");\n");
            }
            _ => unreachable!("validated return type"),
        }
        out.push_str("        } catch (Exception ex) {\n");
        out.push_str("            return AIToolResult.Fail(cid, \"ToolException\", ex.Message);\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
        out.push_str("}\n");
    }
    out
}

/// 包装类名：`__AITool_<Class>_<Method>`（命名空间点替换为下划线）；重名时追加 `_2/_3` 保证唯一。
fn ai_tool_wrapper_names(methods: &[AIToolMethod]) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut names = Vec::with_capacity(methods.len());
    for m in methods {
        let base = format!(
            "__AITool_{}_{}",
            m.class_path.replace('.', "_"),
            m.method_name
        );
        let mut candidate = base.clone();
        let mut n = 2;
        while used.contains(&candidate) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        used.insert(candidate.clone());
        names.push(candidate);
    }
    names
}

/// 由方法签名生成 OpenAI 兼容 parameters schema（D3 用；内嵌于描述符）。
///
/// 标量参数 → 扁平属性；模型参数 → 嵌套 object，属性由模型公开字段/属性的
/// `[Description]` 驱动（`[AITool]` + `[Description]` 组合）。
fn ai_parameters_schema(m: &AIToolMethod) -> String {
    let props: Vec<String> = m
        .params
        .iter()
        .map(|p| {
            if p.model_fields.is_empty() {
                let jt = match p.ty.as_str() {
                    "string" => "string",
                    "int" | "long" => "integer",
                    "double" => "number",
                    "bool" => "boolean",
                    "string[]" => "array",
                    _ => "string",
                };
                let desc = if p.description.is_empty() {
                    String::new()
                } else {
                    format!(",\"description\":\"{}\"", ai_esc(&p.description))
                };
                if jt == "array" {
                    format!(
                        "\"{}\":{{\"type\":\"array\",\"items\":{{\"type\":\"string\"}}{desc}}}",
                        p.name
                    )
                } else {
                    format!("\"{}\":{{\"type\":\"{jt}\"{desc}}}", p.name)
                }
            } else {
                let mut inner: Vec<String> = Vec::new();
                for f in &p.model_fields {
                    let jt = match f.ty.as_str() {
                        "string" => "string",
                        "int" | "long" => "integer",
                        "double" => "number",
                        "bool" => "boolean",
                        _ => "string",
                    };
                    inner.push(format!(
                        "\"{}\":{{\"type\":\"{jt}\",\"description\":\"{}\"}}",
                        f.name,
                        ai_esc(&f.description)
                    ));
                }
                let req: Vec<String> = p
                    .model_fields
                    .iter()
                    .map(|f| format!("\"{}\"", f.name))
                    .collect();
                // 模型参数自身的 `[Description]`（若有）内嵌进嵌套 object，与标量参数一致
                // （RFC 038：每个参数都应有 description 承载，模型参数不例外）。
                let desc = if p.description.is_empty() {
                    String::new()
                } else {
                    format!(",\"description\":\"{}\"", ai_esc(&p.description))
                };
                format!(
                    "\"{}\":{{\"type\":\"object\"{desc},\"properties\":{{{}}},\"required\":[{}]}}",
                    p.name,
                    inner.join(","),
                    req.join(",")
                )
            }
        })
        .collect();
    let required: Vec<String> = m.params.iter().map(|p| format!("\"{}\"", p.name)).collect();
    format!(
        "{{\"type\":\"object\",\"properties\":{{{}}},\"required\":[{}]}}",
        props.join(","),
        required.join(",")
    )
}

/// Arc 字符串字面量转义（嵌入生成源码用）。
fn ai_esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// RFC 038: 若程序含 `[AITool]` 方法，合成 `__AIToolHost` 工具宿主并入编译单元（普通 build / test / publish 均生效）。
fn maybe_inject_ai_tool_host(unit: &mut CompileUnit, obj_dir: Option<&Path>) -> Result<(), String> {
    let tools = collect_ai_tool_methods(&unit.program)?;
    if tools.is_empty() {
        return Ok(());
    }
    let gen_source = generate_ai_tool_host(&tools);
    let gen_path = obj_dir
        .unwrap_or_else(|| Path::new("obj"))
        .join("code")
        .join("ai_tool_host.g.as");
    let gen_file_id = unit.file_registry.allocate(gen_path);
    let gen_program =
        parse::Parser::parse_program_in_file(&gen_source, gen_file_id).map_err(|e| {
            format!("parse generated AI tool host: {e}\n\n-- generated source --\n{gen_source}")
        })?;
    unit.program.items.extend(gen_program.items);
    // RFC 016 M6：在程序入口 Main 首行注入 `__AIToolHost.__RegisterGlobal();`，使编译期
    // 合成的工具宿主注册为 AIHost 默认工具源（实例化 AIHost 即自动获得全部 [AITool] 工具）。
    inject_ai_tool_bootstrap_call(&mut unit.program.items);
    Ok(())
}

/// RFC 016 M6：向顶层 `Main` 函数体首行注入 `__AIToolHost.__RegisterGlobal();`。
/// 仅当程序中存在顶层 `Main` 且可访问时注入；无 Main（动态库）或 Main 在命名空间内时跳过
/// （动态库场景由调用方显式装配，或预留后续 module-init 钩子）。
fn inject_ai_tool_bootstrap_call(items: &mut [ast::Spanned<Item>]) {
    for item in items.iter_mut() {
        if let Item::Fn(f) = &mut item.node {
            if f.name.as_str() == "Main" {
                if let Some(body) = &mut f.body {
                    let call = Expr::MethodCall {
                        receiver: Box::new(ast::Spanned::new(
                            Expr::Ident("__AIToolHost".into()),
                            Span::DUMMY,
                        )),
                        method: "__RegisterGlobal".into(),
                        args: Vec::new(),
                        type_args: Vec::new(),
                        params_span: None,
                    };
                    let stmt = ast::Spanned::new(
                        Stmt::Expr(ast::Spanned::new(call, Span::DUMMY)),
                        Span::DUMMY,
                    );
                    body.stmts.insert(0, stmt);
                }
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RFC 012 声明式贡献聚合 · Inject 自动注册（跨编译单元 · 合成 AddXxx 入口注入）
// ─────────────────────────────────────────────────────────────────────────────

/// 一个 `[Inject]` 标记类：服务键（可空=自注册）+ 生命周期。
struct InjectClass {
    class_path: String,
    service_type: Option<String>,
    lifetime: String,
}

/// 收集当前编译单元内全部 `[Inject]` 标记类（类级属性，跨 namespace 递归）。
/// 支持解析形态：
///   - `[Inject]`                                → 自注册 · Scoped
///   - `[Inject(ServiceLifetime.Singleton)]`     → 自注册 · 指定生命周期
///   - `[Inject(typeof(IService))]`              → 注册为 IService · Scoped
///   - `[Inject(typeof(IService), ServiceLifetime.Transient)]`
///
/// 泛型形态 `[Inject<T>]` 当前解析器不支持（parse_attributes 仅 path + parens），
/// 等价用法 `[Inject(typeof(T))]` 可完全覆盖。
fn collect_inject_classes(program: &ast::Program) -> Vec<InjectClass> {
    let mut classes = Vec::new();
    collect_inject_from_items(&program.items, "", &mut classes);
    classes
}

fn collect_inject_from_items(
    items: &[ast::Spanned<ast::Item>],
    ns_prefix: &str,
    out: &mut Vec<InjectClass>,
) {
    for spanned in items {
        match &spanned.node {
            ast::Item::Class(c) => {
                let path = if ns_prefix.is_empty() {
                    c.name.to_string()
                } else {
                    format!("{ns_prefix}.{}", c.name)
                };
                for attr in &c.attributes {
                    let aname = attr.path.last().map(|i| i.to_string()).unwrap_or_default();
                    if aname != "Inject" && aname != "InjectAttribute" {
                        continue;
                    }
                    let (svc, lifetime) = parse_inject_args(&attr.args);
                    out.push(InjectClass {
                        class_path: path.clone(),
                        service_type: svc,
                        lifetime,
                    });
                }
            }
            ast::Item::Namespace(ns) => {
                let segs: Vec<String> = ns.path.iter().map(|i| i.to_string()).collect();
                let joined = segs.join(".");
                let child = if ns_prefix.is_empty() {
                    joined
                } else if joined.is_empty() {
                    ns_prefix.to_string()
                } else {
                    format!("{ns_prefix}.{joined}")
                };
                collect_inject_from_items(&ns.items, &child, out);
            }
            _ => {}
        }
    }
}

/// 解析 `[Inject]` 属性位置参数 → (service_type, lifetime)。
/// typeof(T) → 服务键；ServiceLifetime.X → 生命周期；缺省 Scoped。
fn parse_inject_args(args: &[ast::AttributeArg]) -> (Option<String>, String) {
    let mut svc = None;
    let mut lifetime = "Scoped".to_string();
    for a in args {
        match a {
            ast::AttributeArg::Type(t) => svc = Some(simple_type_name(&t.node)),
            ast::AttributeArg::MemberPath(p) => {
                if let Some(last) = p.last() {
                    lifetime = last.to_string();
                }
            }
            _ => {}
        }
    }
    (svc, lifetime)
}

/// 简化的具名类型名（路径末段拼接）。
fn simple_type_name(t: &ast::Type) -> String {
    match t {
        ast::Type::Named { path, .. } => path
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("."),
        _ => String::new(),
    }
}

/// 类型是否为 `ServiceCollection`（找 DI 装配点）。
fn type_is_service_collection(t: &ast::Type) -> bool {
    matches!(t, ast::Type::Named { path, .. } if path.last().map(|i| i.to_string()).as_deref() == Some("ServiceCollection"))
}

/// 将全限定名 FQN 构造为 `Type::Named`（多段 path，供 AddXxx 泛型实参直接发射）。
fn fqn_type(fqn: &str) -> ast::Type {
    let path: Vec<ast::Ident> = fqn.split('.').map(|s| s.into()).collect();
    ast::Type::Named {
        path,
        generics: Vec::new(),
    }
}

/// 向顶层 `Main` 中首个 `ServiceCollection` 创建语句之后注入
/// `services.AddXxx<Svc, Impl>();` 语句——通用聚合器直接发射类型化绑定，
/// 使 [Inject] 类在 Build() 前自动入容器（显式静态注册，不合成注册器类）。
/// AddXxx 走既有 ServiceCollectionExtensions → codegen 拦截生成 `__di_factory_TImpl`。
/// 无 ServiceCollection 创建（非 DI 程序）时跳过。
fn inject_inject_bootstrap_call(items: &mut [ast::Spanned<Item>], classes: &[InjectClass]) {
    for item in items.iter_mut() {
        if let Item::Fn(f) = &mut item.node {
            if f.name.as_str() == "Main" {
                if let Some(body) = &mut f.body {
                    let mut idx = None;
                    for (i, s) in body.stmts.iter().enumerate() {
                        if let Stmt::Let { name: _, ty, .. } = &s.node {
                            if ty
                                .as_ref()
                                .map(|t| type_is_service_collection(&t.node))
                                .unwrap_or(false)
                            {
                                idx = Some(i);
                                break;
                            }
                        }
                    }
                    if let Some(i) = idx {
                        let var = match &body.stmts[i].node {
                            Stmt::Let { name, .. } => name.clone(),
                            _ => unreachable!(),
                        };
                        let mut stmts: Vec<ast::Spanned<Stmt>> = Vec::new();
                        for c in classes {
                            let method = match c.lifetime.as_str() {
                                "Singleton" => "AddSingleton",
                                "Transient" => "AddTransient",
                                _ => "AddScoped",
                            };
                            let (svc, impl_) = match &c.service_type {
                                Some(s) => (s.clone(), c.class_path.clone()),
                                None => (c.class_path.clone(), c.class_path.clone()),
                            };
                            let call = Expr::MethodCall {
                                receiver: Box::new(ast::Spanned::new(
                                    Expr::Ident(var.clone()),
                                    Span::DUMMY,
                                )),
                                method: method.into(),
                                args: Vec::new(),
                                type_args: vec![
                                    ast::Spanned::new(fqn_type(&svc), Span::DUMMY),
                                    ast::Spanned::new(fqn_type(&impl_), Span::DUMMY),
                                ],
                                params_span: None,
                            };
                            stmts.push(ast::Spanned::new(
                                Stmt::Expr(ast::Spanned::new(call, Span::DUMMY)),
                                Span::DUMMY,
                            ));
                        }
                        body.stmts.splice(i + 1..i + 1, stmts);
                    }
                }
                break;
            }
        }
    }
}

/// 若程序含 `[Inject]` 类，向 DI 装配点直接发射 AddXxx 绑定。
/// 普通 build / test / publish 均生效。
fn maybe_inject_di_bindings(unit: &mut CompileUnit, _obj_dir: Option<&Path>) -> Result<(), String> {
    let classes = collect_inject_classes(&unit.program);
    if classes.is_empty() {
        return Ok(());
    }
    inject_inject_bootstrap_call(&mut unit.program.items, &classes);
    Ok(())
}

/// 统一入口：编译期合成注入（AI 工具宿主 / DI 绑定）一并执行。
fn maybe_inject_runtime_registries(
    unit: &mut CompileUnit,
    obj_dir: Option<&Path>,
) -> Result<(), String> {
    maybe_inject_ai_tool_host(unit, obj_dir)?;
    maybe_inject_di_bindings(unit, obj_dir)?;
    Ok(())
}

/// RFC 017 D8 v1.0：[`compile_unit`] 的动态库版本——产物为 `.dll`/`.so`/`.dylib`。
///
/// 与 [`compile_unit`] 的差异：
/// - 不要求 `main` 函数（动态库无入口点）
/// - 调用 `codegen::compile_module_to_dynamic_library`（`-shared` + `-fPIC`）
/// - `export_symbols` 列出领域约定符号，Windows MSVC 下显式 `/EXPORT:<symbol>`
fn compile_unit_to_dynamic_library(
    unit: &mut CompileUnit,
    source: &str,
    source_file: &Path,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    release: bool,
    debug_info: bool,
    native_lib_paths: &[PathBuf],
    export_symbols: &[String],
    package_meta: Option<codegen::PackageMeta>,
    options: &CompileOptions,
    emitter: &dyn ArtifactEmitter,
) -> Result<(), String> {
    // RFC 038/012：声明式 [AITool] + [Inject] → 合成注册器（动态库同样生效）。
    maybe_inject_runtime_registries(unit, obj_dir)?;
    // RFC 027 ResX CodeGen：.resx → 强类型访问器注入（动态库同样生效）。
    maybe_inject_resx_accessors(unit, source_file, obj_dir)?;
    let prepared = prepare_compilation(unit, options, false)?;

    if let Some(out) = output {
        let triple = target.map(|t| t.as_str());
        // 产物发射（P5）走装备接口：亲 `codegen::compile_module_to_dynamic_library` 由默认装备委托。
        let diags = emitter
            .emit(ArtifactRequest {
                role: EmitRole::DynamicLibrary,
                fns: &prepared.mir_fns,
                layouts: &prepared.layouts,
                output: out,
                obj_dir,
                target: triple,
                release,
                file_path: &prepared.file_path,
                source,
                debug_info,
                fn_spans: &prepared.fn_spans,
                native_modules: &prepared.native_modules,
                native_lib_paths,
                external_symbols: &prepared.external_symbols,
                project_kind: codegen::ProjectKind::Library,
                export_symbols,
                package_meta,
                keep_ir: options.keep_ir,
            })
            .map_err(|e| format!("codegen error: {e}"))?;
        render_static_init_diagnostics(&diags);
    }

    emit_doc_xml(unit, output, obj_dir)?;
    Ok(())
}

/// RFC 017：生成 `.xml` 文档产物（默认语言，C# DocComment 规范）。
///
/// docgen 只读 AST（`unit.program`，原始未 desugar），不依赖 MIR/typeck。
///
/// 生成 `.xml` 文档产物——对标 C# `dotnet build` / `<GenerateDocumentationFile>`。
///
/// .NET 体系：XML 作为中间产物写入 `obj/`；最终复制到 `bin/` 与二进制同目录。
/// 多语言本地化遵循 `<package>.<locale>.xml` 命名（如 `Arc.zh-CN.xml`）。
///
/// **红线**：禁止将 .xml 写入源码树——始终使用 obj_dir/bin 输出目录。
fn emit_doc_xml(
    unit: &CompileUnit,
    output: Option<&PathBuf>,
    obj_dir: Option<&Path>,
) -> Result<(), String> {
    let package_name = derive_package_name(unit);
    let doc_xml = codegen::docgen::generate_doc_xml(&unit.program, &package_name);

    // 1. 中间产物：始终写入 obj/（对标 .NET obj/<Config>/<TFM>/<Assembly>.xml）
    let obj_dir = obj_dir.unwrap_or_else(|| Path::new("obj"));
    std::fs::create_dir_all(obj_dir).map_err(|e| format!("create obj dir failed: {e}"))?;
    let obj_path = obj_dir.join(format!("{}.xml", package_name));
    std::fs::write(&obj_path, &doc_xml).map_err(|e| format!("write .xml doc failed: {e}"))?;

    // 2. 最终产物：若 output 存在则同步到 bin/<package>.xml（与二进制同目录）
    if let Some(out) = output {
        if let Some(bin_dir) = out.parent() {
            // 防御：若 output 为裸文件名，parent() 返回空路径会导致 XML 写入 CWD。
            if bin_dir.as_os_str().is_empty() {
                return Ok(());
            }
            std::fs::create_dir_all(bin_dir).map_err(|e| format!("create bin dir failed: {e}"))?;
            let bin_path = bin_dir.join(format!("{}.xml", package_name));
            std::fs::write(&bin_path, &doc_xml)
                .map_err(|e| format!("write .xml doc to bin failed: {e}"))?;
        }
    }
    Ok(())
}

/// 从 CompileUnit 推导包名（用于 .xml 文档的 assembly name）。
/// 优先从 arc.toml 获取 `[package].name`；其次使用入口文件 stem。
fn derive_package_name(unit: &CompileUnit) -> String {
    // 优先从 manifest 获取包名
    if let Some((_, manifest)) = crate::manifest::find_arc_manifest(&unit.root) {
        if !manifest.package.name.is_empty() {
            return manifest.package.name.clone();
        }
    }
    // 兜底：入口文件名 stem
    unit.root
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "arc-std".into())
}

/// 公共编译准备：parse → hir → typeck → borrowck → mir，产出 MIR + layouts + exports。
struct PreparedCompilation {
    mir_fns: Vec<(String, mir::MirCfgBody)>,
    layouts: typeck::ProgramLayouts,
    fn_spans: HashMap<String, Span>,
    native_modules: Vec<ast::NativeModule>,
    file_path: String,
    /// RFC 017 M4-link Phase B：跨 `.aopkg` 包外部符号列表。
    ///
    /// 从 `CompileUnit.external_symbols` 直接搬运。codegen 消费此列表发射
    /// `declare <ret> @<symbol>(...)`，让链接器从 lib.o 解析定义。无 `.aopkg`
    /// 依赖时为空切片，codegen emit_external_decls 跳过 declare 段。
    external_symbols: Vec<typeck::ExternalSymbolEntry>,
}

/// 阶段边界 panic 兜底：将编译核心阶段内部的可达 panic 收敛为诊断错误，
/// 避免用户输入在 typeck / mir 阶段触发编译器裸崩溃（panic unwinding 到 CLI
/// 边界 → 未格式化崩溃 / 栈回溯）。
///
/// 语义：正常路径原样透传 `f()` 的返回值；panic 路径返回
/// `Err("internal compiler error during <phase>: <msg>")`，原始 panic 消息
/// 保留在诊断串中便于排障。仅作**防御性**兜底——不改变既有诊断语义
/// （`TypeError` 等仍走原有 `?` 传播），也不吞掉正常错误路径。
fn phase_guard<T>(phase: &str, f: impl FnOnce() -> T) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown internal panic".to_string());
            Err(format!("internal compiler error during {phase}: {msg}"))
        }
    }
}

fn prepare_compilation(
    unit: &CompileUnit,
    options: &CompileOptions,
    mark_all_classes_weak: bool,
) -> Result<PreparedCompilation, String> {
    let mut program = unit.program.clone();
    eprintln!(
        "[STAGE] prepare_compilation start, items={}",
        program.items.len()
    );
    let desugar_errors = hir::desugar_program(&mut program);
    if !desugar_errors.is_empty() {
        return Err(desugar_errors.join("\n"));
    }
    // RFC 044：yield 迭代器脱糖（LINQ 之后——lambda 产物在迭代器体内被 M1 拒绝）。
    let yield_errors = hir::desugar_yield_program(&mut program);
    if !yield_errors.is_empty() {
        return Err(yield_errors.join("\n"));
    }

    let mut hir_builder = hir::HirBuilder::new();
    eprintln!("[STAGE] hir lower start");
    let module = hir_builder
        .lower_program(&program)
        .map_err(|e| format!("hir error: {e}"))?;
    eprintln!("[STAGE] hir lower done");

    let mut typeck = typeck::TypeChecker::new();
    // RFC 017 M4-link Phase B：注入 LinkonceOdr 类名集合，使
    // `fn_linkage_for_class` 能自动标记集合内类型为 linkonce_odr（跨 .o 去重）。
    //
    // 非 publish 构建：扫描 `CompileUnit.program.items` 中 file_id 位于
    // `std/` 目录或 `[dependencies]` path 依赖源码目录下的所有
    // class/struct/interface/enum/variant 定义，收集其名称。
    // 链接器在跨 .o 场景（main.o + lib.o 均 `using Arc;`）自动去重。
    //
    // publish（LibraryObject）：整包弱符号（RFC 017 §D2.1「库 .o 全局表
    // linkonce_odr，被消费方主程序强符号覆盖」）。lib .o 会内嵌依赖库源码
    // （Lib.o 内嵌 Util），与依赖库自身 .o（Util.o）同时链接时，同一类型
    // 的两份定义必须同为 COMDAT 才能被 lld 折叠——任一份为强符号即
    // `lld-link: error: duplicate symbol`。故 publish 全量标记。
    {
        use ast::Item;
        let workspace = crate::loader::find_workspace_root(&unit.root);
        // 规范化 std_dir 以匹配 file_registry 路径的格式差异（Windows \\?\ 前缀、`..` 等）
        // RFC 031 §8：尊重项目 `[std].path` 覆盖；完整解析链（SDK 捆绑 std / 环境变量兜底）。
        let std_dir =
            if let Some((manifest_dir, m)) = crate::manifest::find_arc_manifest(&unit.root) {
                crate::manifest::resolve_effective_std_root(
                    &workspace,
                    Some(&manifest_dir),
                    m.std.as_ref(),
                )
            } else {
                crate::manifest::resolve_effective_std_root(&workspace, None, None)
            };
        // RFC 017 M4-link：`[dependencies]` path 依赖源码目录（相对项目根）同样视为
        // 弱符号来源——`arc build` 无 `.aopkg` 时的源码回退与 `arc publish` 内嵌
        // 依赖源码都依赖此集合避免与依赖自身 `.aopkg` 的定义重复。
        let mut weak_dirs: Vec<std::path::PathBuf> =
            vec![std::fs::canonicalize(&std_dir).unwrap_or(std_dir)];
        if let Some((manifest_dir, m)) = crate::manifest::find_arc_manifest(&unit.root) {
            for spec in m.dependencies.values() {
                if !spec.path.is_empty() {
                    let dir = manifest_dir.join(&spec.path);
                    weak_dirs.push(std::fs::canonicalize(&dir).unwrap_or(dir));
                }
            }
        }
        let mut std_class_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // 递归提取 items 列表中所有类名（含 namespace 内嵌套的类型）
        fn collect_class_names(
            items: &[ast::Spanned<Item>],
            weak_dirs: &[std::path::PathBuf],
            mark_all_weak: bool,
            file_registry: &FileRegistry,
            out: &mut std::collections::HashSet<String>,
        ) {
            for item in items {
                if !mark_all_weak {
                    // 规范化比较：消除 Windows \\?\ 前缀、`.`、`..` 等路径格式差异
                    let is_weak_src = file_registry
                        .path_of(item.span.file_id)
                        .and_then(|p| std::fs::canonicalize(p).ok())
                        .is_some_and(|p| weak_dirs.iter().any(|d| p.starts_with(d)));
                    if !is_weak_src {
                        continue;
                    }
                }
                match &item.node {
                    Item::Class(c) => {
                        out.insert(c.name.to_string());
                    }
                    Item::Struct(s) => {
                        out.insert(s.name.to_string());
                    }
                    Item::Interface(i) => {
                        out.insert(i.name.to_string());
                    }
                    Item::Enum(e) => {
                        out.insert(e.name.to_string());
                    }
                    Item::Variant(v) => {
                        out.insert(v.name.to_string());
                    }
                    Item::Namespace(ns) => {
                        collect_class_names(
                            &ns.items,
                            weak_dirs,
                            mark_all_weak,
                            file_registry,
                            out,
                        );
                    }
                    _ => {}
                }
            }
        }

        collect_class_names(
            &unit.program.items,
            &weak_dirs,
            mark_all_classes_weak,
            &unit.file_registry,
            &mut std_class_names,
        );
        typeck.set_std_class_names(std_class_names);
        // RFC 025 M2：注入 FileId → 包名，启用跨包 internal 硬拒绝。
        typeck.set_file_packages(unit.file_packages.clone(), Some(unit.entry_package.clone()));
        // RFC 025 M2+：注入 InternalsVisibleTo 映射，放行指定包访问 internal。
        typeck.set_internals_visible_to(unit.internals_visible_to.clone());
    }
    // RFC 016 M1: 注册 native 契约模块到 TypeRegistry（在 check_module 之前，
    // 使 libc.puts(...) 等调用能被 check_native_method 分派）。
    // `check_module` 重建 registry 后会通过 `reregister_native_modules`
    // 自动重注册缓存中的 native 模块。
    typeck.register_native_modules(&unit.native_modules);
    // RFC 017 M4-link Phase B：注册 `.aopkg` 依赖包的外部符号到 TypeRegistry
    // （在 check_module 之前，使跨包类型引用能命中 registry）。
    // `check_module` 重建 registry 后会通过 `reregister_external_symbols`
    // 自动重注册缓存中的外部符号。无 `.aopkg` 依赖时 `external_symbols` 为空，无副作用。
    typeck.register_external_symbols(&unit.external_symbols);
    eprintln!("[STAGE] typeck check_module start");
    phase_guard("typeck check_module", || {
        typeck.check_module(&module).map_err(|es| {
            es.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
    })??;

    // RFC 005 里程碑④：编译期声明级字段环 warning（arc-cycle-001）打印到 stderr。
    // warning 不阻断编译（exit 0）；`field_cycle_policy = "off"` 时静默。
    if options.field_cycle_policy == FieldCyclePolicy::Warn {
        for w in typeck.warnings() {
            eprintln!("{}", w.render());
        }
    }

    // RFC 009：Pass 3（宏展开 / Source Generator）+ Pass 4（完整 typeck）。
    // 无宏容器与生成器时为 no-op；Pass 4 会向 typed_fns 追加宏容器与生成代码。
    eprintln!("[STAGE] typeck run_pass3 start");
    phase_guard("typeck run_pass3", || {
        typeck.run_pass3().map_err(|es| {
            es.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
    })??;
    phase_guard("typeck run_pass4", || {
        typeck.run_pass4().map_err(|es| {
            es.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
    })??;
    eprintln!("[STAGE] typeck run_pass4 done");
    // 静态字段初始化器中的泛型函数调用（如 RegisterProperty<string>）
    // 必须在 typed_fns 收集之前强制实例化——静态字段 init 以原始 AST 存储，
    // 不经过 check_expr 路径，因此不会自动触发 instantiate_generic_fn。
    {
        let layouts = typeck::layouts_from_registry(typeck.registry());
        let static_fields: Vec<_> = layouts.static_fields.clone();
        for sf in &static_fields {
            if let Some(ref init_expr) = sf.init {
                if let Expr::Call {
                    ref func,
                    ref type_args,
                    ..
                } = init_expr.node
                {
                    if let Expr::Ident(func_name) = &func.node {
                        let type_ids: Vec<TypeId> = type_args
                            .iter()
                            .map(|t| {
                                if let Type::Named { ref path, .. } = t.node {
                                    match path.last().map(|i| i.as_str()).unwrap_or("void") {
                                        "int" => TypeId::Int,
                                        "long" => TypeId::Long,
                                        "short" => TypeId::Short,
                                        "byte" => TypeId::Byte,
                                        "uint" => TypeId::UInt,
                                        "ushort" => TypeId::UShort,
                                        "sbyte" => TypeId::SByte,
                                        "char" => TypeId::Char,
                                        "bool" => TypeId::Bool,
                                        "float" => TypeId::Float,
                                        "double" => TypeId::Double,
                                        "string" => TypeId::String,
                                        "object" => TypeId::Object,
                                        "void" => TypeId::Void,
                                        other => TypeId::Named(other.into()),
                                    }
                                } else {
                                    TypeId::Void
                                }
                            })
                            .collect();
                        // Ignore errors — if instantiation fails, it was already
                        // handled by the normal typeck path (or the function doesn't exist).
                        let _ = typeck.force_instantiate_generic_fn(func_name, &type_ids);
                    }
                }
            }
        }
    }

    // RFC 037 M-D0：`[Observable]` auto-property 的 setter 由 codegen 合成，发射
    // `@Signal_<T>_Set` / `@__ctor::Signal_<T>` 调用。若用户源码未显式引用
    // `Signal<T>`（无 `ObserveProperty` 调用），typeck 不会自动单态化该泛型类，
    // 导致 tree-shake 后 `Signal_<T>` 方法缺失（LLVM undefined value）。故对每个
    // `[Observable]` 属性按 backing field 类型在收集 typed_fns 之前强制实例化
    // `Signal_<T>`（与上面静态字段初始化器泛型函数强制实例化同构）。
    {
        let observable_props = typeck.observable_properties();
        if !observable_props.is_empty() {
            for (owner, member) in observable_props {
                let field_ty = typeck
                    .registry()
                    .types
                    .get(&owner)
                    .and_then(|nom| nom.fields.get(&member))
                    .map(|f| f.ty.clone())
                    .unwrap_or_default();
                if field_ty.is_empty() {
                    continue;
                }
                let type_id = type_name_to_type_id(&field_ty);
                // Ignore errors — Signal 模板来自 std，未 using 时无 `Signal` 模板，
                // 该属性不影响 Signal_<T> 单态化（codegen 亦不会发射该通道）。
                let _ = typeck.force_instantiate_generic_class(&"Signal".into(), &[type_id]);
            }
        }
    }

    let typed_fns = typeck.typed_fns().to_vec();

    // 借用检查：typed HIR（typeck 产物），非 MIR CFG。
    let mut borrowck = typeck::BorrowChecker::new();
    borrowck.check_module(&module, &typed_fns).map_err(|es| {
        es.iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    eprintln!("[STAGE] pre-MIR done (about to lower)");
    let expr_types = if std::env::var("ARC_DISABLE_EXPR_TYPES").is_ok() {
        typeck::ExprTypeTable::default()
    } else {
        typeck.take_expr_type_table()
    };
    let mut mir_fns = phase_guard("mir lower", || {
        mir::lower_module(&typed_fns, typeck.registry(), &expr_types)
    })?;
    eprintln!("[STAGE] mir lower done");
    if std::env::var("ARC_DEBUG_MIRFNS").is_ok() {
        for (n, _) in &mir_fns {
            if n.contains("Weak") {
                eprintln!("[mirfn] {n}");
            }
        }
    }

    // C2 单态化完整性：MIR 级泛型方法/构造器单态化（`try_create_mono_body` /
    // `generate_generic_class_ctors` / `generate_generic_class_methods`）克隆出的
    // body 会引用具体泛型类（如 `Element.SetValue<int>` 克隆体内的
    // `new Signal<int>()` / `Signal_int::Set`），而 typeck 在类型解析路径可能未
    // 实例化该类（用户源码无显式 `Signal<int>` 注解）→ `layouts_from_registry`
    // 缺该类布局 → codegen 字段访问回退 `(16, "int")` 产生错位与 LLVM 类型错误
    // （`'%t11' defined with type 'i32' but expected 'ptr'`）。
    //
    // 修复：扫描 lowered MIR 引用的类名，对 registry 缺失的泛型类实例强制
    // 单态化，并**重新 lowering**——新实例化类的实例方法/构造器以真实 typed
    // body 落地（其 itable/vtable/typeinfo 所需方法体才能齐备），而非仅有
    // registry 条目。循环至无新增类（fixpoint）。
    {
        let mut guard = 0;
        loop {
            let mut class_refs: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            mir::collect_concrete_class_refs(&mir_fns, typeck.registry(), &mut class_refs);
            let mut added = false;
            for class in &class_refs {
                if typeck.registry().types.contains_key(class.as_str()) {
                    continue;
                }
                let Some((template, args, _)) =
                    mir::resolve_generic_class_template_by_name(class, typeck.registry())
                else {
                    continue;
                };
                if args.is_empty() {
                    continue;
                }
                // 忽略实例化失败（如模板不在 registry）。
                if typeck
                    .force_instantiate_generic_class(&template, &args)
                    .is_ok()
                {
                    added = true;
                }
            }
            if !added {
                break;
            }
            guard += 1;
            if guard > 64 {
                return Err(
                    "MIR lower: force-instantiate generic classes exceeded fixpoint limit; \
                     possible cyclic generic class instantiation"
                        .to_string(),
                );
            }
            // 重新 lowering：新实例化类的 typed fns 一并入队，方法体正确落地。
            let typed_fns = typeck.typed_fns().to_vec();
            mir_fns = phase_guard("mir lower (re-lower)", || {
                mir::lower_module(&typed_fns, typeck.registry(), &expr_types)
            })?;
        }
    }

    let mut layouts = typeck::layouts_from_registry(typeck.registry());
    // RFC 037 M-D0：`[Observable]` auto-property 集合——AttributeTable
    // `has_attr(def_id, "Observable")` 查询（typeck 侧注册），供 codegen
    // FieldSet 发射点合成「相等性短路 + 隐藏通知通道」。
    layouts.observable_properties = typeck.observable_properties();

    // RFC 006「接口泛型方法分派」：从 MIR 收集接口泛型方法实例化，
    // 填充 `InterfaceLayout.generic_instances`（实例化槽位名）供 codegen
    // itable 发射与查找共享。必须在 tree-shaking 之前完成——tree-shaker
    // 需据此 force-keep mono body。
    {
        let iface_insts = mir::collect_iface_generic_instances(&mir_fns, typeck.registry());
        for (iface, insts) in &iface_insts {
            let Some(il) = layouts.interfaces.get_mut(iface.as_str()) else {
                continue;
            };
            for (method, suffix) in insts {
                let inst_name = format!("{method}__{suffix}");
                if !il.generic_instances.contains(&inst_name) {
                    il.generic_instances.push(inst_name);
                }
            }
        }
    }

    // Tree-shaking: filter unreachable MIR functions before codegen.
    // RFC 016：`load != static` 的模块，其探测函数与抛异常辅助仅被 codegen 生成的
    // 懒解析器/间接调用引用（Arc 调用图不可达）——强制保留（filter 内 force-keep）。
    let keep_fns = runtime_load_keep_fns(&unit.native_modules);
    let template_fns = typeck.generic_template_names();
    if std::env::var("ARC_DEBUG_TEMPLATES").is_ok() {
        eprintln!("[templates] total={}", template_fns.len());
        let mut v: Vec<_> = template_fns.iter().cloned().collect();
        v.sort();
        for t in v.iter().take(60) {
            eprintln!("[template] {t}");
        }
    }
    let mir_fns = filter_reachable_mir_fns(mir_fns, &layouts, &keep_fns, &template_fns);
    if std::env::var("ARC_DEBUG_DUMP_IMAGE").is_ok() {
        for (n, body) in &mir_fns {
            if n.contains("Image_EnsureLoaded") {
                eprintln!("==== DUMP {n} locals={:?}", body.locals);
                for (bn, blk) in &body.blocks {
                    eprintln!("-- block {bn:?}");
                    for s in &blk.statements {
                        eprintln!("    {s:?}");
                    }
                }
            }
        }
    }
    if std::env::var("ARC_DEBUG_MIRFNS").is_ok() {
        for (n, _) in &mir_fns {
            if n.contains("Weak") {
                eprintln!("[mirfn] {n}");
            }
        }
    }

    // RFC 036 §2.5 / RFC 036 / RFC 005 §2.3：NLL 借用检查——无条件启用
    //（`nll_strict` 恒 true，⑤ 已移除 CLI 开关）。
    // 在 MIR 上运行 `run_nll_check_module`，非空诊断 → 编译失败。
    if options.nll_strict {
        let diags = mir::dataflow::run_nll_check_module(&mir_fns);
        if !diags.is_empty() {
            let msgs: Vec<String> = diags
                .iter()
                .map(|d| format!("{}: {}", d.code.code_str(), d.message))
                .collect();
            return Err(format!("NLL borrow check failed:\n{}", msgs.join("\n")));
        }
    }

    // MIR 字段访问验证 pass——无条件启用（对齐 NLL 契约）：在 codegen 之前
    // 拦截一切 `(class, field)` 不可解析引用，非空诊断 → 编译失败。
    let field_diags = mir::field_check::run_field_check_module(&mir_fns, typeck.registry());
    if !field_diags.is_empty() {
        let msgs: Vec<String> = field_diags
            .iter()
            .map(|d| format!("{}: {}（函数 {}）", d.code.code_str(), d.message, d.fn_name))
            .collect();
        return Err(format!("Field check failed:\n{}", msgs.join("\n")));
    }

    let fn_spans = build_fn_span_map(&program);
    let file_path = unit.root.display().to_string();

    Ok(PreparedCompilation {
        mir_fns,
        layouts,
        fn_spans,
        native_modules: unit.native_modules.clone(),
        file_path,
        external_symbols: unit.external_symbols.clone(),
    })
}

/// Build a map from MIR function name → definition span by walking AST items.
///
/// MIR function names are:
/// - Top-level functions: `FnDef.name` (e.g., "Main")
/// - Class constructors: `__ctor::{ClassName}` (e.g., "__ctor::Holder")
/// - Class methods: `MethodDef.sig.name` (e.g., "Translate")
///
/// Auto-generated property getters/setters (`get_X`/`set_X`) and lifted
/// lambdas (`__lambda_*`) don't have AST spans — they'll fall back to
/// `Span::DUMMY` (line 0) in the debug table.
fn build_fn_span_map(program: &ast::Program) -> HashMap<String, Span> {
    let mut map = HashMap::new();
    collect_fn_spans(&program.items, &mut map);
    map
}

fn collect_fn_spans(items: &[ast::Spanned<Item>], map: &mut HashMap<String, Span>) {
    for item in items {
        match &item.node {
            Item::Fn(f) => {
                map.entry(f.name.to_string()).or_insert(item.span);
            }
            Item::Class(c) => {
                for ctor in &c.constructors {
                    // 类构造函数重载 mangle：无参为 __ctor::Class，有参为 __ctor::Class_N；
                    // 同参数量碰撞时按签名 `__ctor::Class_<arity>_<p0>...` 消歧
                    // （与 typeck check_class 的 ctor_link_name 决策一致）。
                    let ctor_arity = ctor.node.params.len();
                    let collision = ctor_arity > 0
                        && c.constructors
                            .iter()
                            .filter(|c2| c2.node.params.len() == ctor_arity)
                            .count()
                            > 1;
                    let key = if ctor_arity == 0 {
                        format!("__ctor::{}", c.name)
                    } else if collision {
                        let params: Vec<String> = ctor
                            .node
                            .params
                            .iter()
                            .map(|p| {
                                typeck::type_id_to_field_name(&ast_type_to_type_id(&p.ty.node))
                                    .to_string()
                            })
                            .collect();
                        format!("__ctor::{}_{}_{}", c.name, ctor_arity, params.join("_"))
                    } else {
                        format!("__ctor::{}_{}", c.name, ctor_arity)
                    };
                    map.entry(key).or_insert(ctor.span);
                }
                for method in &c.methods {
                    map.entry(method.node.sig.name.to_string())
                        .or_insert(method.span);
                }
            }
            Item::Namespace(ns) => {
                collect_fn_spans(&ns.items, map);
            }
            _ => {}
        }
    }
}

// ===========================================================================
// Release tree-shaking: BFS-based reachability filtering of MIR functions.
// ===========================================================================

fn collect_mir_edges(
    body: &mir::MirCfgBody,
    caller_id: u32,
    name_to_id: &HashMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    for block in body.blocks.values() {
        for stmt in &block.statements {
            collect_stmt_edges(stmt, caller_id, name_to_id, edges);
        }
        collect_terminator_edges(&block.terminator, caller_id, name_to_id, edges);
    }
}

fn collect_stmt_edges(
    stmt: &mir::MirStatement,
    caller_id: u32,
    name_to_id: &HashMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    match stmt {
        mir::MirStatement::Assign { rvalue, .. } => {
            collect_rvalue_edges(rvalue, caller_id, name_to_id, edges);
        }
        mir::MirStatement::Drop(_) => {}
        mir::MirStatement::Return(Some(rv)) => {
            collect_rvalue_edges(rv, caller_id, name_to_id, edges);
        }
        mir::MirStatement::Return(None) => {}
        mir::MirStatement::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_operand_edges(cond, caller_id, name_to_id, edges);
            for s in then_body {
                collect_stmt_edges(s, caller_id, name_to_id, edges);
            }
            for s in else_body {
                collect_stmt_edges(s, caller_id, name_to_id, edges);
            }
        }
        mir::MirStatement::While { cond, body, .. } => {
            collect_rvalue_edges(cond, caller_id, name_to_id, edges);
            for s in body {
                collect_stmt_edges(s, caller_id, name_to_id, edges);
            }
        }
        mir::MirStatement::FieldSet { object, value, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
            collect_rvalue_edges(value, caller_id, name_to_id, edges);
        }
        // RFC 006 M3：静态字段写入——无 operand 依赖，仅 value 贡献边。
        mir::MirStatement::StaticFieldSet { value, .. } => {
            collect_rvalue_edges(value, caller_id, name_to_id, edges);
        }
        mir::MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            collect_operand_edges(array, caller_id, name_to_id, edges);
            collect_operand_edges(index, caller_id, name_to_id, edges);
            collect_rvalue_edges(value, caller_id, name_to_id, edges);
        }
        mir::MirStatement::LinqForeach { body, .. } => {
            for s in body {
                collect_stmt_edges(s, caller_id, name_to_id, edges);
            }
        }
        mir::MirStatement::Await { task, .. } => {
            collect_rvalue_edges(task, caller_id, name_to_id, edges);
        }
        mir::MirStatement::Throw { value } => {
            collect_rvalue_edges(value, caller_id, name_to_id, edges);
        }
        mir::MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_stmt_edges(s, caller_id, name_to_id, edges);
            }
            for s in catch_body {
                collect_stmt_edges(s, caller_id, name_to_id, edges);
            }
        }
        mir::MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_stmt_edges(s, caller_id, name_to_id, edges);
            }
            for s in finally {
                collect_stmt_edges(s, caller_id, name_to_id, edges);
            }
        }
        mir::MirStatement::Break | mir::MirStatement::Continue => {}
    }
}

/// 收集 CFG 基本块终结符携带的操作数引用边。
///
/// `to_cfg()` 把顶层 `return <expr>` 展平为 `MirTerminator::Return(Some(op))`
/// （`mir::MirRvalue::Use(op)` 直接透传为操作数，不落 `Assign` 语句）；因此
/// `return () => …`（`FnPtr`/`Closure` 操作数）与 `if (<delegate>)` 条件里的
/// 闭包/函数指针引用边**不在 `statements` 里**，须从终结符单独收集。否则
/// `return <capturing-lambda>` 直返路径的 `__lambda_rt_*` 被 tree-shake 剪除
/// → LLVM `use of undefined value '@__lambda_rt_N'`。
fn collect_terminator_edges(
    term: &mir::MirTerminator,
    caller_id: u32,
    name_to_id: &HashMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    match term {
        mir::MirTerminator::Return(Some(op)) | mir::MirTerminator::Throw(op) => {
            collect_operand_edges(op, caller_id, name_to_id, edges);
        }
        mir::MirTerminator::CondBr { cond, .. } => {
            collect_operand_edges(cond, caller_id, name_to_id, edges);
        }
        mir::MirTerminator::Goto(_)
        | mir::MirTerminator::Return(None)
        | mir::MirTerminator::Unreachable => {}
    }
}

/// 将类型名（`FieldInfo.ty`，如 `int` / `string` / `Foo`）映射为 `TypeId`。
///
/// 供 `[Observable]` 强制实例化 `Signal_<T>` 时使用——与 `ast` 侧
/// `TypeId::from_name` 的基元识别一致，类类型落 `Named`。
fn type_name_to_type_id(name: &str) -> ast::TypeId {
    match name {
        "int" => ast::TypeId::Int,
        "long" => ast::TypeId::Long,
        "short" => ast::TypeId::Short,
        "byte" => ast::TypeId::Byte,
        "char" => ast::TypeId::Char,
        "float" => ast::TypeId::Float,
        "double" => ast::TypeId::Double,
        "bool" => ast::TypeId::Bool,
        "uint" => ast::TypeId::UInt,
        "ulong" => ast::TypeId::ULong,
        "ushort" => ast::TypeId::UShort,
        "sbyte" => ast::TypeId::SByte,
        "string" => ast::TypeId::String,
        "void" => ast::TypeId::Void,
        "object" => ast::TypeId::Object,
        other => ast::TypeId::Named(other.into()),
    }
}

fn collect_rvalue_edges(
    rv: &mir::MirRvalue,
    caller_id: u32,
    name_to_id: &HashMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    match rv {
        mir::MirRvalue::Use(op) => collect_operand_edges(op, caller_id, name_to_id, edges),
        mir::MirRvalue::Call { func, args } => {
            push_edge(func, caller_id, name_to_id, edges);
            for arg in args {
                collect_operand_edges(arg, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::MethodCall {
            target_fn,
            args,
            receiver,
            receiver_type,
            method,
            ..
        } => {
            // `target_fn` 为 None 时（宿主类方法调用，如 `self.DispatchTest(idx)`），
            // 回退到 `receiver_type::method` 构造按名边，否则 tree-shake 会误剪
            // Reachability 保留的宿主方法（如并行路径的 QifTestHost::DispatchTest）。
            let name = target_fn
                .clone()
                .or_else(|| Some(format!("{receiver_type}::{method}")));
            if let Some(name) = name {
                push_edge(&name, caller_id, name_to_id, edges);
            }
            collect_operand_edges(receiver, caller_id, name_to_id, edges);
            for arg in args {
                collect_operand_edges(arg, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::NullCondMethod {
            target_fn,
            args,
            receiver,
            default,
            ..
        } => {
            if let Some(name) = target_fn {
                push_edge(name, caller_id, name_to_id, edges);
            }
            collect_operand_edges(receiver, caller_id, name_to_id, edges);
            for arg in args {
                collect_operand_edges(arg, caller_id, name_to_id, edges);
            }
            collect_operand_edges(default, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::ForceDerefMethod {
            target_fn,
            args,
            receiver,
            ..
        } => {
            if let Some(name) = target_fn {
                push_edge(name, caller_id, name_to_id, edges);
            }
            collect_operand_edges(receiver, caller_id, name_to_id, edges);
            for arg in args {
                collect_operand_edges(arg, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::New {
            class,
            args,
            ctor_params,
        } => {
            // 构造函数采用重载 mangle：无参为 __ctor::Class，有参为 __ctor::Class_N
            // (N = args.len())。MIR 判定同参数量碰撞时（ctor_params 非空）按签名
            // `__ctor::Class_<arity>_<p0>...` 消歧。
            let ctor_name = if ctor_params.is_empty() {
                if args.is_empty() {
                    format!("__ctor::{class}")
                } else {
                    format!("__ctor::{class}_{}", args.len())
                }
            } else {
                format!(
                    "__ctor::{class}_{}_{}",
                    ctor_params.len(),
                    ctor_params.join("_")
                )
            };
            push_edge(&ctor_name, caller_id, name_to_id, edges);
            for arg in args {
                collect_operand_edges(arg, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::Binary { left, right, .. } => {
            collect_operand_edges(left, caller_id, name_to_id, edges);
            collect_operand_edges(right, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::FieldGet { object, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::MakeIface { object, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::MakeIfaceDyn { object, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::AdaptIface { object, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::StructLit { fields, .. } => {
            for (_, op) in fields {
                collect_operand_edges(op, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::ArrayLit { elements, .. } => {
            for elem in elements {
                match elem {
                    mir::ArrayLitElement::Value(rv) => {
                        collect_rvalue_edges(rv, caller_id, name_to_id, edges);
                    }
                    mir::ArrayLitElement::Spread(op) => {
                        collect_operand_edges(op, caller_id, name_to_id, edges);
                    }
                }
            }
        }
        mir::MirRvalue::IndexGet { array, index, .. } => {
            collect_operand_edges(array, caller_id, name_to_id, edges);
            collect_operand_edges(index, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::FnPtr { name } => {
            push_edge(name, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::IndirectCall { func, args } => {
            collect_operand_edges(func, caller_id, name_to_id, edges);
            for arg in args {
                collect_operand_edges(arg, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::Coalesce { left, right } => {
            collect_operand_edges(left, caller_id, name_to_id, edges);
            collect_operand_edges(right, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            collect_operand_edges(cond, caller_id, name_to_id, edges);
            collect_operand_edges(then_val, caller_id, name_to_id, edges);
            collect_operand_edges(else_val, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            collect_operand_edges(receiver, caller_id, name_to_id, edges);
            collect_operand_edges(default, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::ForceDerefField { receiver, .. } => {
            collect_operand_edges(receiver, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::Box { src, .. } => {
            collect_operand_edges(src, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::Unbox { src, .. } => {
            collect_operand_edges(src, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(p) = payload {
                collect_operand_edges(p, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::VariantTag { scrutinee, .. } => {
            collect_operand_edges(scrutinee, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::VariantExtract { scrutinee, .. } => {
            collect_operand_edges(scrutinee, caller_id, name_to_id, edges);
        }
        // RFC 009 D3 SoA：SoaFieldGet 边收集——穿透 array/index 子操作数，
        // 避免 SoA 表达式丢失可达性边。codegen 已实现 emit_soa_field_get。
        mir::MirRvalue::SoaFieldGet { array, index, .. } => {
            collect_operand_edges(array, caller_id, name_to_id, edges);
            collect_operand_edges(index, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            collect_operand_edges(array, caller_id, name_to_id, edges);
            if let Some(s) = start {
                collect_operand_edges(s, caller_id, name_to_id, edges);
            }
            if let Some(l) = length {
                collect_operand_edges(l, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::SpanFromStack { elements, .. } => {
            for e in elements {
                collect_operand_edges(e, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            collect_operand_edges(span, caller_id, name_to_id, edges);
            collect_operand_edges(start, caller_id, name_to_id, edges);
            if let Some(l) = length {
                collect_operand_edges(l, caller_id, name_to_id, edges);
            }
        }
        mir::MirRvalue::SpanFill { span, value, .. } => {
            collect_operand_edges(span, caller_id, name_to_id, edges);
            collect_operand_edges(value, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::SpanClear { span, .. } => {
            collect_operand_edges(span, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::SpanCopyTo { src, dest, .. } => {
            collect_operand_edges(src, caller_id, name_to_id, edges);
            collect_operand_edges(dest, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            collect_operand_edges(src, caller_id, name_to_id, edges);
            collect_operand_edges(dest, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::SpanToArray { span, .. } => {
            collect_operand_edges(span, caller_id, name_to_id, edges);
        }
        mir::MirRvalue::LinqChain(_) | mir::MirRvalue::ExpressionTreeConst { .. } => {}
        mir::MirRvalue::NewArray { length, .. } => {
            collect_operand_edges(length, caller_id, name_to_id, edges);
        }
    }
}

fn collect_operand_edges(
    op: &mir::MirOperand,
    caller_id: u32,
    name_to_id: &HashMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    match op {
        mir::MirOperand::FnPtr { name } => {
            push_edge(name, caller_id, name_to_id, edges);
        }
        mir::MirOperand::Closure { fn_name, env } => {
            push_edge(fn_name, caller_id, name_to_id, edges);
            for (_, operand) in env {
                collect_operand_edges(operand, caller_id, name_to_id, edges);
            }
        }
        mir::MirOperand::Field { object, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirOperand::Iface { object, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirOperand::UnboxIface { object, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirOperand::UnboxString { object } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirOperand::UnboxGeneric { object, .. } => {
            collect_operand_edges(object, caller_id, name_to_id, edges);
        }
        mir::MirOperand::Local(_)
        | mir::MirOperand::ConstInt(_)
        | mir::MirOperand::ConstFloat(_)
        | mir::MirOperand::ConstString(_)
        | mir::MirOperand::ConstBool(_)
        | mir::MirOperand::ConstNull
        | mir::MirOperand::ConstDefault { .. }
        | mir::MirOperand::AddrOf(_)
        | mir::MirOperand::TypeId { .. }
        | mir::MirOperand::TypeInfoPtr { .. }
        | mir::MirOperand::StaticField { .. } => {}
    }
}

fn push_edge(
    callee_name: &str,
    caller_id: u32,
    name_to_id: &HashMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    if let Some(&callee_id) = name_to_id.get(callee_name) {
        edges.push(Edge::new(
            caller_id,
            callee_id,
            EdgeKind::Call,
            0,
            0,
            0,
            true,
        ));
    }
}

/// CD-10/D1/CD-11/D2：沿类解析接口/虚方法在本类的最终实现链接名（含重载后缀）。
/// 与 codegen `resolve_method_impl` 同构：精确签名键优先，按名回退。
fn resolve_class_method_link(
    layouts: &typeck::ProgramLayouts,
    class: &typeck::ClassLayout,
    method: &str,
    params: &[ast::Ident],
) -> Option<String> {
    if let Some(impl_class) = class.method_impl.get(&(method.into(), params.to_vec())) {
        if let Some(entry) = layouts.classes.get(impl_class.as_str()).and_then(|c| {
            c.declared_methods
                .iter()
                .find(|m| m.name.as_str() == method && m.param_types == params)
        }) {
            return Some(entry.link_name.clone());
        }
    }
    let mut cur = class.name.as_str();
    loop {
        let cl = layouts.classes.get(cur)?;
        if let Some(entry) = cl
            .declared_methods
            .iter()
            .find(|m| m.name.as_str() == method)
        {
            return Some(entry.link_name.clone());
        }
        let Some(p) = &cl.parent else {
            return None;
        };
        cur = p.as_str();
    }
}

/// RFC 004 P0 Phase 2：struct 接口方法在本 struct 的最终实现链接名（签名键）。
/// 与 codegen `resolve_struct_method` 同构：精确签名键优先，按名回退；struct 无父链。
fn resolve_struct_method_link(
    layouts: &typeck::ProgramLayouts,
    s: &typeck::StructLayout,
    method: &str,
    params: &[ast::Ident],
) -> Option<String> {
    if let Some(impl_struct) = s.method_impl.get(&(method.into(), params.to_vec())) {
        if let Some(entry) = layouts.structs.get(impl_struct.as_str()).and_then(|st| {
            st.declared_methods
                .iter()
                .find(|m| m.name.as_str() == method && m.param_types == params)
        }) {
            return Some(entry.link_name.clone());
        }
    }
    if let Some(entry) = s
        .declared_methods
        .iter()
        .find(|m| m.name.as_str() == method)
    {
        return Some(entry.link_name.clone());
    }
    None
}

fn build_vdispatch_groups(
    layouts: &typeck::ProgramLayouts,
    name_to_id: &HashMap<String, u32>,
) -> Vec<VirtualDispatchGroup> {
    let mut groups: Vec<VirtualDispatchGroup> = Vec::new();

    // Interface virtual dispatch: interface method → all implementing class methods.
    for (iface_name, iface_layout) in &layouts.interfaces {
        for (method_name, _ret_ty, _) in &iface_layout.methods {
            let iface_method = format!("{iface_name}::{method_name}");
            let iface_id = match name_to_id.get(&iface_method) {
                Some(&id) => id,
                None => continue,
            };
            let mut impl_ids: Vec<u32> = Vec::new();
            for (class_name, class_layout) in &layouts.classes {
                if !class_layout.interfaces.contains(iface_name) {
                    continue;
                }
                let class_method = format!("{class_name}::{method_name}");
                if let Some(&id) = name_to_id.get(&class_method) {
                    impl_ids.push(id);
                }
            }
            if !impl_ids.is_empty() {
                groups.push(VirtualDispatchGroup::new(iface_id, impl_ids));
            }
        }
    }

    // Class inheritance virtual dispatch: base class virtual method → all
    // overriding subclass methods.  Without this, overrides like
    // Circle::Kind / Square::Kind are tree-shaken because only the base
    // Shape::Kind appears in the MIR call graph (virtual call via vtable
    // resolves to the base slot, not each override).
    //
    // The base (declaring) class must be located by walking the *full* parent
    // chain, not just the immediate parent.  For
    // `VisualHost : ContentControl : ... : Element`, the immediate parent
    // ContentControl only *inherits* `IsDataContextBoundary` (declared in
    // Element).  Basing the group on the immediate parent makes
    // `name_to_id["ContentControl::IsDataContextBoundary"]` miss, so the group
    // is skipped and `VisualHost::IsDataContextBoundary` is tree-shaken —
    // codegen then emits a wrong `return false` stub for it (visualhost_dc
    // isolation leak: inner DataContext inherits host's into the boundary).
    for (_class_name, class_layout) in &layouts.classes {
        let Some(parent_name) = &class_layout.parent else {
            continue;
        };
        // Walk up the parent chain to collect all virtual slots.
        let Some(parent_layout) = layouts.classes.get(parent_name) else {
            continue;
        };
        for ps in &parent_layout.virtual_slots {
            // CD-10/D1：签名槽。派生类槽位（name+params 匹配）的 link_name 即
            // 最派生 override 符号；基类槽位 link_name 为最上声明类符号。
            let Some(cs) = class_layout
                .virtual_slots
                .iter()
                .find(|s| s.name == ps.name && s.params == ps.params)
            else {
                continue;
            };
            // Only add groups where the override differs from the base
            // (i.e. the subclass doesn't just inherit the base implementation).
            if cs.link_name == ps.link_name {
                continue;
            }
            let base_id = match name_to_id.get(&ps.link_name) {
                Some(&id) => id,
                None => continue,
            };
            let override_id = match name_to_id.get(&cs.link_name) {
                Some(&id) => id,
                None => continue,
            };
            if override_id != base_id {
                groups.push(VirtualDispatchGroup::new(base_id, vec![override_id]));
            }
        }
    }

    groups
}

/// 从 MIR body 收集 `new Class(args)` 站点（class 名 + ctor arity）。
fn collect_new_sites(body: &mir::MirCfgBody, out: &mut Vec<(String, usize)>) {
    for block in body.blocks.values() {
        for stmt in &block.statements {
            let rvalue = match stmt {
                mir::MirStatement::Assign { rvalue, .. }
                | mir::MirStatement::FieldSet { value: rvalue, .. }
                | mir::MirStatement::StaticFieldSet { value: rvalue, .. } => Some(rvalue),
                mir::MirStatement::Return(Some(rvalue)) => Some(rvalue),
                _ => None,
            };
            if let Some(rv) = rvalue {
                collect_new_from_rvalue(rv, out);
            }
        }
    }
}

fn collect_new_from_rvalue(rv: &mir::MirRvalue, out: &mut Vec<(String, usize)>) {
    match rv {
        mir::MirRvalue::New { class, args, .. } => {
            out.push((class.clone(), args.len()));
        }
        mir::MirRvalue::ArrayLit { elements, .. } => {
            for elem in elements {
                if let mir::ArrayLitElement::Value(inner) = elem {
                    collect_new_from_rvalue(inner, out);
                }
            }
        }
        _ => {}
    }
}

/// RFC 023 M1：收集 DI 方式1 注册的实现类型（`new ServiceDescriptor(typeof(TService),
/// typeof(TImpl), lt)` 的 `typeof(TImpl)` 操作数）。codegen 据此生成
/// `__di_factory_{TImpl}`，其 `call @__ctor::{TImpl}` 不在 MIR 可达图中，
/// tree-shaker 须按此强制保留默认 ctor。
fn collect_di_impl_types(body: &mir::MirCfgBody, out: &mut Vec<String>) {
    for block in body.blocks.values() {
        for stmt in &block.statements {
            let rvalue = match stmt {
                mir::MirStatement::Assign { rvalue, .. }
                | mir::MirStatement::FieldSet { value: rvalue, .. }
                | mir::MirStatement::StaticFieldSet { value: rvalue, .. } => Some(rvalue),
                mir::MirStatement::Return(Some(rvalue)) => Some(rvalue),
                _ => None,
            };
            if let Some(rv) = rvalue {
                collect_di_types_from_rvalue(rv, out);
            }
        }
    }
}

fn collect_di_types_from_rvalue(rv: &mir::MirRvalue, out: &mut Vec<String>) {
    match rv {
        mir::MirRvalue::New { class, args, .. } => {
            // 方式1 特征：args[1] 为 typeof(TImpl)（MirOperand::TypeId）。
            if class == "ServiceDescriptor" {
                if let Some(mir::MirOperand::TypeId { type_name }) = args.get(1) {
                    out.push(type_name.clone());
                }
            }
        }
        mir::MirRvalue::ArrayLit { elements, .. } => {
            for elem in elements {
                if let mir::ArrayLitElement::Value(inner) = elem {
                    collect_di_types_from_rvalue(inner, out);
                }
            }
        }
        _ => {}
    }
}

/// P1（async 取消异常通道）：检测 MIR body 是否调用
/// `CancellationToken.ThrowIfCancellationRequested`。codegen 将该调用反糖为
/// `if (canceled) throw new OperationCanceledException()`（emit_call.rs
/// try_emit_ct_method），异常 ctor 链须 force-keep（见 `filter_reachable_mir_fns`）。
fn body_uses_ct_throw(body: &mir::MirCfgBody) -> bool {
    body_contains_rvalue(body, |rv| {
        matches!(
            rv,
            mir::MirRvalue::MethodCall { method, .. } if method == "ThrowIfCancellationRequested"
        )
    })
}

/// ALC Entry 调用点检测：`receiver_type == "Assembly" && method == "Entry"`，
/// 与 codegen `try_emit_assembly_entry` 准入条件对偶（emit_call.rs）。
fn body_uses_assembly_entry(body: &mir::MirCfgBody) -> bool {
    body_contains_rvalue(body, |rv| {
        matches!(
            rv,
            mir::MirRvalue::MethodCall { method, receiver_type, .. }
                if method == "Entry" && receiver_type == "Assembly"
        )
    })
}

fn body_contains_rvalue(body: &mir::MirCfgBody, pred: impl Fn(&mir::MirRvalue) -> bool) -> bool {
    body.blocks
        .values()
        .any(|b| b.statements.iter().any(|s| stmt_contains_rvalue(s, &pred)))
}

fn stmt_contains_rvalue(stmt: &mir::MirStatement, pred: &impl Fn(&mir::MirRvalue) -> bool) -> bool {
    match stmt {
        mir::MirStatement::Assign { rvalue, .. }
        | mir::MirStatement::Return(Some(rvalue))
        | mir::MirStatement::Throw { value: rvalue }
        | mir::MirStatement::FieldSet { value: rvalue, .. }
        | mir::MirStatement::StaticFieldSet { value: rvalue, .. }
        | mir::MirStatement::IndexSet { value: rvalue, .. }
        | mir::MirStatement::Await { task: rvalue, .. } => pred(rvalue),
        mir::MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(|s| stmt_contains_rvalue(s, pred))
                || else_body.iter().any(|s| stmt_contains_rvalue(s, pred))
        }
        mir::MirStatement::While { body, .. } => body.iter().any(|s| stmt_contains_rvalue(s, pred)),
        mir::MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            try_body.iter().any(|s| stmt_contains_rvalue(s, pred))
                || catch_body.iter().any(|s| stmt_contains_rvalue(s, pred))
        }
        mir::MirStatement::TryFinally { body, finally } => {
            body.iter().any(|s| stmt_contains_rvalue(s, pred))
                || finally.iter().any(|s| stmt_contains_rvalue(s, pred))
        }
        mir::MirStatement::LinqForeach { body, .. } => {
            body.iter().any(|s| stmt_contains_rvalue(s, pred))
        }
        _ => false,
    }
}

/// 从 `ClassLayout.parent` 返回 `[class, parent, grandparent, …]`。
fn class_ancestor_chain(layouts: &typeck::ProgramLayouts, class: &str) -> Vec<String> {
    let mut chain = vec![class.to_string()];
    let mut current: ast::Ident = class.into();
    while let Some(cl) = layouts.classes.get(&current) {
        let Some(parent) = cl.parent.clone() else {
            break;
        };
        chain.push(parent.to_string());
        current = parent;
    }
    chain
}

/// RFC 016：返回需要强制保留的 MIR 函数名——仅被 codegen 生成的懒解析器 /
/// 间接调用引用、但 Arc 调用图不可达的函数：
///
/// - `Native::ThrowIfUnavailable`：间接调用失败路径抛 `NativeLibraryNotFoundException`
///   的 std 辅助（生效策略非 static 的模块存在时保留）。
///
/// 对 `auto` 模块即使编译期分流为 static，也保守保留（死函数无害）。
fn runtime_load_keep_fns(modules: &[ast::NativeModule]) -> Vec<String> {
    let mut out = Vec::new();
    let has_runtime_like = modules.iter().any(|m| m.load != ast::LoadStrategy::Static);
    if has_runtime_like {
        out.push("Native::ThrowIfUnavailable".to_string());
    }
    out
}

/// RFC 006 A3 S3：收集静态字段初始化器引用的方法 MIR 名（`Class::method`）。
///
/// 静态字段初值由 codegen 注入的 `__sinit_<Class>` / `__lazy_init_<Class>`
/// helper 调用（惰性 readonly 在首次访问时、急切 static 在模块初始化时）。
/// MIR 调用图无对应 Call 边 → 这些方法会被 tree-shake 剪除，导致 LLVM
/// `use of undefined value`。把它们 force-keep，保证发射（其依赖随后经
/// `expand_reachable_callees` 闭包一并保留）。
fn collect_static_init_keep_fns(layouts: &typeck::ProgramLayouts) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for sf in &layouts.static_fields {
        let Some(init) = &sf.init else {
            continue;
        };
        collect_static_init_expr_methods(&init.node, sf.class.as_str(), &mut out);
    }
    out
}

/// 递归遍历静态字段初始化器表达式，把其中调用的方法记为 `Class::method`。
fn collect_static_init_expr_methods(expr: &Expr, class: &str, out: &mut Vec<String>) {
    match expr {
        // 裸静态方法调用：`Construct()` 在类 C 字段初值中 → MIR 名 `C::Construct`。
        Expr::Call { func, args, .. } => {
            if let Expr::Ident(callee) = &func.node {
                let key = format!("{class}::{callee}");
                if !out.contains(&key) {
                    out.push(key);
                }
            }
            for arg in args {
                collect_static_init_expr_methods(&arg.node, class, out);
            }
            collect_static_init_expr_methods(&func.node, class, out);
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            // RFC 006 A3 S6a：静态方法调用 `Class.Method(...)` 的初始化器——方法仅由
            // codegen 静态初始化器调用（MIR 无 Call 边），需 force-keep，否则被
            // tree-shake 剪除 → LLVM undefined value。receiver 为类 Ident 时记录
            // `Class::Method`。
            if let Expr::Ident(rcv) = &receiver.node {
                let key = format!("{rcv}::{method}");
                if !out.contains(&key) {
                    out.push(key);
                }
            }
            collect_static_init_expr_methods(&receiver.node, class, out);
            for arg in args {
                collect_static_init_expr_methods(&arg.node, class, out);
            }
        }
        Expr::New { ty, args, .. } => {
            // RFC 006 A3 S6a：`static readonly X = new T(...)` 中 T 的 ctor 仅由
            // codegen 静态初始化器调用（MIR 无 Call 边），需 force-keep，否则被
            // tree-shake 剪除 → LLVM undefined value。ctor MIR 名按 arity 后缀：
            // `__ctor::T`（无参）/ `__ctor::T_{arity}`（有参）。T 为单态化类名
            //（泛型经 mangle_generic），与 codegen `emit_static_new_expr` 一致。
            let new_class = static_init_new_class(&ty.node);
            let ctor_key = if args.is_empty() {
                format!("__ctor::{new_class}")
            } else {
                format!("__ctor::{new_class}_{}", args.len())
            };
            if !out.contains(&ctor_key) {
                out.push(ctor_key);
            }
            for arg in args {
                collect_static_init_expr_methods(&arg.node, class, out);
            }
        }
        Expr::Field { receiver, .. } => {
            collect_static_init_expr_methods(&receiver.node, class, out);
        }
        Expr::Binary { left, right, .. } => {
            collect_static_init_expr_methods(&left.node, class, out);
            collect_static_init_expr_methods(&right.node, class, out);
        }
        Expr::Unary { expr: inner, .. } => {
            collect_static_init_expr_methods(&inner.node, class, out);
        }
        Expr::Index { receiver, index } => {
            collect_static_init_expr_methods(&receiver.node, class, out);
            collect_static_init_expr_methods(&index.node, class, out);
        }
        Expr::Await(inner) => {
            collect_static_init_expr_methods(&inner.node, class, out);
        }
        _ => {}
    }
}

fn filter_reachable_mir_fns(
    mir_fns: Vec<(String, mir::MirCfgBody)>,
    layouts: &typeck::ProgramLayouts,
    keep_fns: &[String],
    template_fns: &std::collections::HashSet<String>,
) -> Vec<(String, mir::MirCfgBody)> {
    // RFC 012 S6 A1：泛型模板是编译期蓝图，其方法体引用未单态化的类型参数
    // 符号（如 `Weak_T_GetWeakSlot`），无独立可发射的运行期 body——仅单态化
    // 实例才有合法 body。此处无条件剔除（可执行构建中模板不可达本会被
    // tree-shake 剪除；`--dynamic` 库无 Main/Entry 全量保留，必须显式剔除）。
    //
    // **例外**：stub-handled 模板（Weak/List/Dictionary 等泛型方法的 stub）
    // 的 IR 由 `emit_stubs` 直接生成（不依赖 MIR body），且可能被发射的
    // 单态化实例引用（如 `AssemblyLoadContext` 调用 `Weak_T_GetWeakSlot`）。
    // 一并剔除会致 stub 缺失 → `--dynamic` 库 `undefined value @Weak_T_*`
    // （`--dynamic` 库构建实测）。故保留 stub-handled 名，
    // 使其在 fns 发射遍历中经 `try_emit_stub` 生成 stub。
    let mir_fns: Vec<_> = mir_fns
        .into_iter()
        .filter(|(name, _)| {
            if !template_fns.contains(name) {
                return true;
            }
            !codegen::is_builtin_stub_fn(name)
        })
        .collect();
    if mir_fns.is_empty() {
        return mir_fns;
    }
    let name_to_id: HashMap<String, u32> = mir_fns
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (name.clone(), i as u32))
        .collect();

    let mut edges: Vec<Edge> = Vec::new();
    for (caller_name, body) in &mir_fns {
        let caller_id = name_to_id[caller_name.as_str()];
        collect_mir_edges(body, caller_id, &name_to_id, &mut edges);
    }

    let entry_points: Vec<EntryPoint> = mir_fns
        .iter()
        .filter(|(name, _)| {
            name.eq_ignore_ascii_case("main")
                || name == "Entry"
                || name.ends_with("::Main")
                || name.ends_with("::Entry")
        })
        .map(|(name, _)| {
            let id = name_to_id[name.as_str()];
            EntryPoint::new(id, EntryPointKind::Main, 0)
        })
        .collect();

    if entry_points.is_empty() {
        return mir_fns;
    }

    // caller → callees 邻接表：供 itable/dict 强制保留后的传递闭包扩展使用
    //（须在 `with_edges` 消费 `edges` 之前建好）。
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for e in &edges {
        adj.entry(e.caller_symbol_id)
            .or_default()
            .push(e.callee_symbol_id);
    }

    let vdispatch_groups = build_vdispatch_groups(layouts, &name_to_id);
    let universe: Vec<u32> = (0..mir_fns.len() as u32).collect();
    let input = AnalysisInput::new()
        .with_entry_points(entry_points)
        .with_edges(edges)
        .with_universe(universe)
        .with_virtual_dispatch_groups(vdispatch_groups);

    let report = reachability::analyze(&input);
    let mut reachable: HashSet<u32> = report
        .reference_graph
        .reachable_symbols
        .iter()
        .copied()
        .collect();

    // 内置 stub 类方法（`is_builtin_stub_fn`，List/Dictionary/Weak/FileStream/
    // TextBuffer 等）由 `emit_stubs` 直接生成 IR，可能经**合成路径**被引用——
    // 类属性 custom-accessor 访问在 `emit_cfg` 按 `mangle_method` 直调
    // `@<Class>_get_<P>`、vtable/itable 槽、`emit_itables` 等，这些路径在 MIR
    // 调用图中**无 Call 边**，无法由 BFS 触发保守传播。若不强制保留会被
    // tree-shake → `arc-prune-001`（**TextBuffer_get_LineCount 复现**：code
    // 编辑器行号读取 `[Builtin]` 属性 LineCount，走 custom-accessor getter
    // 直调，MIR 无调用边 → 被剪除 → 调用体仍 `call` 其符号而 stub 未发射）。
    // 与 CD-32 接口属性 getter force-keep 同构，但覆盖全部内置 stub 方法
    //（不只 getter），消除同类合成路径缺口。stub IR 为 linkonce_odr+comdat
    // 去重、体积小，整体保留无冗余；stub 若引用其他符号经下方 callee 闭包
    // 扩展一并保留。
    for (i, (name, _)) in mir_fns.iter().enumerate() {
        if codegen::is_builtin_stub_fn(name) {
            reachable.insert(i as u32);
        }
    }
    if std::env::var("ARC_DEBUG_REACH").is_ok() {
        for (n, id) in &name_to_id {
            if n.contains("Weak")
                || n.contains("RegisterWeakReference")
                || n.contains("GetWeakSlot")
            {
                eprintln!("[reach] {n} id={id} reachable={}", reachable.contains(id));
            }
        }
        for (n, body) in &mir_fns {
            if n == "__lambda_rt_86" || n == "__QifTestHost::DispatchTest" {
                for (bn, blk) in &body.blocks {
                    for s in &blk.statements {
                        eprintln!("[reach-body] {n} block={} stmt={s:?}", bn.0);
                    }
                }
            }
        }
    }

    // 接口 itable 槽引用的实现方法必须保留：接口方法本身无 TypedFn，
    // `build_vdispatch_groups` 无法以 iface_id 触发保守传播；若不强制保留，
    // `@.itable.Class_Iface` 会引用已被剪除的 `@Class_Method`（LLVM undefined）。
    for class in layouts.classes.values() {
        for iface_name in &class.interfaces {
            let Some(ilayout) = layouts.interfaces.get(iface_name) else {
                continue;
            };
            for (mname, _, iface_params) in &ilayout.methods {
                // CD-11/D2：按签名解析最终实现，key 为完整链接名（含重载后缀）。
                let Some(key) = resolve_class_method_link(layouts, class, mname, iface_params)
                else {
                    continue;
                };
                if let Some(&id) = name_to_id.get(&key) {
                    reachable.insert(id);
                }
            }
            // RFC 006：泛型方法实例化槽位引用的 mono body 必须保留。
            // 泛型方法不进 vtable（不可 virtual/override），impl_class 即 class 自身。
            for inst_name in &ilayout.generic_instances {
                let key = format!("{}::{}", class.name, inst_name);
                if let Some(&id) = name_to_id.get(&key) {
                    reachable.insert(id);
                }
            }
            // CD-32 根因：接口**属性**槽（`ilayout.properties`）引用的 getter 实现
            // 同样必须保留——`emit_itables` 的 properties 循环按 `resolve_method_impl`
            // 解析 `get_{P}` 真实实现并写入 itable 槽；若被 tree-shake 剪除，
            // `fn_names` 缺失 → codegen 走 `emit_iface_property_getter` 合成兜底
            // （按类字段名找属性失败 → fallback 偏移 16 读 `__state` → 运行时崩溃；
            // yield 状态机 `IAsyncEnumerator.Current` 即此路径，async_stream_e2e /
            // arc_ai_{agnes,openai}_provider_e2e 的 SSE 工具增量解析全灭根因）。
            // 与上方 methods 槽 force-keep 同构（属性 getter 无直接调用边，MIR
            // 调用图不可达）。
            for (pname, _) in &ilayout.properties {
                let getter_name = format!("get_{pname}");
                let Some(key) = resolve_class_method_link(layouts, class, &getter_name, &[]) else {
                    continue;
                };
                if let Some(&id) = name_to_id.get(&key) {
                    reachable.insert(id);
                }
            }
        }
    }

    // RFC 004 P0 Phase 2：struct 接口 itable 槽引用的实现方法必须保留。struct 方法
    // 仅经装箱接口分派（`IShape i = s; i.Area()` 经值接收者 thunk 间接调用，MIR 无
    // Call 边），若不强制保留会被 tree-shake → `@.itable.{Struct}_Box_{Iface}` 引用
    // undefined `@Struct_Method`（LLVM 链接失败/静默空 itable）。
    for s in layouts.structs.values() {
        for iface_name in &s.interfaces {
            let Some(ilayout) = layouts.interfaces.get(iface_name) else {
                continue;
            };
            for (mname, _, iface_params) in &ilayout.methods {
                let Some(key) = resolve_struct_method_link(layouts, s, mname, iface_params) else {
                    continue;
                };
                if let Some(&id) = name_to_id.get(&key) {
                    reachable.insert(id);
                }
            }
            // CD-32（struct 侧对称）：接口属性槽引用的 struct getter 同样保留。
            for (pname, _) in &ilayout.properties {
                let getter_name = format!("get_{pname}");
                let Some(key) = resolve_struct_method_link(layouts, s, &getter_name, &[]) else {
                    continue;
                };
                if let Some(&id) = name_to_id.get(&key) {
                    reachable.insert(id);
                }
            }
        }
    }

    // 类 vtable 槽引用的实现方法必须保留：基类抽象方法（abstract property getter
    // 或 abstract method）无方法体、不在 MIR fns 中，`build_vdispatch_groups` 无法
    // 以基类槽为触发点保守传播 → 派生类 override 被 tree-shake → codegen 只发射
    // 返回默认值的 stub（CD-5：`AIToolStreamHandler.Name` 抽象属性 override
    // `AIToolSandbox` 读取为 null → 工具注册失败）。与 itable force-keep 同构：
    // vtable 全局按 `virtual_slots` 发射槽位，槽位引用的实现必须保留。
    for class in layouts.classes.values() {
        for slot in &class.virtual_slots {
            // 槽位 link_name 已含最派生实现类与重载消歧后缀。
            if let Some(&id) = name_to_id.get(&slot.link_name) {
                reachable.insert(id);
            }
        }
    }

    // RFC 004 M2：`Dictionary_<K,_>` / `ConcurrentDictionary_<K,_>` 用户类型键的
    // `K::GetHashCode` / `K::Equals` 仅被 codegen trampoline 以函数指针交给
    // runtime（MIR 无 Call 边）。若不强制保留，tree-shake 会剪掉它们，
    // 导致 `@__dict_eq_{K}` 引用 undefined `@K_Equals`。
    for class_name in layouts.classes.keys() {
        let Some(k) = dict_user_key_type(class_name.as_str()) else {
            continue;
        };
        for m in ["GetHashCode", "Equals"] {
            let key = format!("{k}::{m}");
            if let Some(&id) = name_to_id.get(&key) {
                reachable.insert(id);
            }
        }
    }

    // RFC 016：`load != static` 的 native 模块——抛异常辅助
    // （Native::ThrowIfUnavailable）仅被 codegen 生成的懒解析器 /
    // 间接调用失败路径引用，Arc 调用图不可达。force-keep 后经下方 callee
    // 闭包扩展，其依赖（如 NativeLibraryNotFoundException ctor）一并保留。
    for keep in keep_fns {
        if let Some(&id) = name_to_id.get(keep.as_str()) {
            reachable.insert(id);
        }
    }

    // RFC 006 A3 S3：静态字段初始化器引用的方法 force-keep（MIR 无 Call 边，
    // 否则被剪除 → LLVM undefined value）。其依赖经下方 callee 闭包扩展保留。
    for keep in collect_static_init_keep_fns(layouts) {
        if let Some(&id) = name_to_id.get(keep.as_str()) {
            reachable.insert(id);
        }
    }

    // RFC 037 M-D0：`[Observable]` 属性（auto-property + custom-accessor）的
    // 合成 setter / 通知路径由 codegen 以 raw IR 发射（MIR 无 Call 边）：
    // - auto-property 合成 setter / `NotifyPropertyChanged` 变换 → `@Signal_<T>_Set`
    //   / `@__ctor::Signal_<T>` 调用；按属性类型保留 `Signal_<T>::Set` 与
    //   `__ctor::Signal_<T>`。
    // - custom-accessor `NotifyPropertyChanged(<Name>)` 变换读当前值 → 调
    //   `@<Class>_get_<Name>`（无同名 backing field，走 getter）。若 getter 未被
    //   用户源码引用（如仅赋值不读取），tree-shake 会剪掉它 → LLVM undefined
    //   value。强制保留 custom-accessor 属性的 getter。
    for (owner, member) in &layouts.observable_properties {
        let class = layouts.classes.get(owner.as_str());
        let is_custom_accessor = class
            .map(|c| !c.fields.iter().any(|f| f.name == *member))
            .unwrap_or(false);
        if is_custom_accessor {
            let getter_key = format!("{owner}::get_{member}");
            if let Some(&id) = name_to_id.get(&getter_key) {
                reachable.insert(id);
            }
        }
        let prop_ty = class
            .and_then(|c| c.declared_properties.iter().find(|p| p.name == *member))
            .map(|p| p.property_type.as_str())
            .unwrap_or_default();
        if prop_ty.is_empty() {
            continue;
        }
        let signal_class = format!("Signal_{prop_ty}");
        for key in [
            format!("{signal_class}::Set"),
            format!("__ctor::{signal_class}"),
        ] {
            if let Some(&id) = name_to_id.get(&key) {
                reachable.insert(id);
            }
        }
    }

    // P1（async 取消异常通道）：`ct.ThrowIfCancellationRequested()` 在 codegen
    // 反糖为 `if (canceled) throw new OperationCanceledException()`（emit_call.rs
    // try_emit_ct_method），异常对象经 `emit_new` 分配并调用异常 ctor 链——MIR
    // 无 `New` 边（catch 场景仅引用类型做 rt_obj_isa），tree-shake 会剪掉
    // `__ctor::OperationCanceledException`（及基类 ctor）→ emit_new 引用 undefined
    // / 空 stub，取消异常无法构造。故检测到该调用即 force-keep 异常 ctor 链。
    if mir_fns.iter().any(|(_, body)| body_uses_ct_throw(body)) {
        for ctor in [
            "__ctor::OperationCanceledException",
            "__ctor::SystemException",
            "__ctor::Exception",
        ] {
            if let Some(&id) = name_to_id.get(ctor) {
                reachable.insert(id);
            }
        }
    }

    // P2（ALC Entry 异常通道）：`asm.Entry<T>()` 调用点由 codegen 反糖为
    // `rt_library_sym` 间接调用，NULL 分支合成 `throw new
    // EntryPointNotFoundException(msg)`（emit_call.rs try_emit_assembly_entry）。
    // 该异常 ctor 仅经 emit_new 引用，MIR 调用图无此 Call 边（宿主源从不显式
    // 引用该类型），tree-shake 会剪掉 `__ctor::EntryPointNotFoundException_1`
    // → 完整性门报 arc-prune-001（u5_entry_call_roundtrip e2e 实证）。与上方
    // P1 同构：检测到 Entry 调用点即 force-keep 异常 ctor，基类 ctor 链
    // （SystemException / Exception）由下方闭包扩展沿 Call 边带入。
    if mir_fns
        .iter()
        .any(|(_, body)| body_uses_assembly_entry(body))
    {
        if let Some(&id) = name_to_id.get("__ctor::EntryPointNotFoundException_1") {
            reachable.insert(id);
        }
    }

    // itable / dict 强制保留插在 BFS 之后，不会沿 Call 边传播。对当前
    // reachable 集合再做一次 callee 闭包扩展，避免保留方法体却剪掉其
    // `__ctor::Exception_1` 等被调用符号（di_abstractions / ServiceProvider）。
    // `__lambda_rt_*` 已由 FnPtr/Closure 边进入 BFS；不可再 OR 强制保留，
    // 否则 `--filter Convert_*` 会留下 LazyTests 的 lambda 却剪掉 Bump。
    let expand_reachable_callees = |reachable: &mut HashSet<u32>| {
        let mut queue: VecDeque<u32> = reachable.iter().copied().collect();
        while let Some(id) = queue.pop_front() {
            let Some(callees) = adj.get(&id) else {
                continue;
            };
            for &callee in callees {
                if reachable.insert(callee) {
                    queue.push_back(callee);
                }
            }
        }
    };
    expand_reachable_callees(&mut reachable);

    // RFC 037 M3 UI：`emit_new` 仅调用 `__ctor::Derived[_N]`，不链接基类 ctor。
    // 保留 `new T()` 目标类及其继承链上全部 ctor MIR（如 Element 初始化
    // Children），避免 tree-shake 后以空 stub 替代 → Element.AddChild AV。
    let mut new_sites: Vec<(String, usize)> = Vec::new();
    for (i, (_, body)) in mir_fns.iter().enumerate() {
        if reachable.contains(&(i as u32)) {
            collect_new_sites(body, &mut new_sites);
        }
    }
    for (class, arity) in new_sites {
        for ancestor in class_ancestor_chain(layouts, &class) {
            if let Some(&id) = name_to_id.get(format!("__ctor::{ancestor}").as_str()) {
                reachable.insert(id);
            }
            if arity > 0 {
                let overloaded = format!("__ctor::{ancestor}_{arity}");
                if let Some(&id) = name_to_id.get(overloaded.as_str()) {
                    reachable.insert(id);
                }
            }
        }
    }
    // RFC 023 M1 DI 方式1：`new ServiceDescriptor(typeof(TService), typeof(TImpl), lt)`
    // 的 codegen 注入会生成 `__di_factory_{TImpl}` 并 `call @__ctor::{TImpl}`。
    // factory 是 codegen 产物（非 MIR fn），其调用边不在 reachability 图中——
    // 若 TImpl 默认 ctor 仅被 factory 引用，tree-shake 会剪掉它 → LLVM
    // `use of undefined value '@__ctor_{TImpl}'`（probe11 复现）。此处从
    // 可达 MIR body 提取方式1 实现类型，强制保留其默认 ctor。
    let mut di_impl_types: Vec<String> = Vec::new();
    for (i, (_, body)) in mir_fns.iter().enumerate() {
        if reachable.contains(&(i as u32)) {
            collect_di_impl_types(body, &mut di_impl_types);
        }
    }
    for ty in di_impl_types {
        // ctor 重载 mangle（check_class.rs 的 ctor_link_name）：无参 ctor
        // `__ctor::Class`，有参 ctor `__ctor::Class_<arity>`（arity = 形参个数，
        // 排除 this）；同参数量碰撞时按签名 `__ctor::Class_<arity>_<p0>...` 消歧。
        // 工厂的构造器选择策略（RFC 023 冲刺批次二：参数最多 / 唯一超集）在
        // emit_di 单点实现，pipeline 先于 codegen 无法预知选择结果——保守保留
        // 该类**全部** ctor 的可达性，避免在 pipeline 侧双轨复现选择逻辑造成
        // 漂移（只保留首个 ctor 是 M1 旧限制，已随批次二失效）。键名经
        // `ctor_link_name` 与 check_class/emit_di 共享同一 mangle 决策：仅按
        // arity 生成会在碰撞类上匹配不到真实签名键名 → 参数化 ctor 被剪除 →
        // LLVM undefined value（di_sprint_multi_ctor_superset_tiebreak 复现）。
        let ctor_keys = match layouts.classes.get(&ty as &str) {
            Some(c) if !c.constructors.is_empty() => {
                let ctors = &c.constructors;
                ctors
                    .iter()
                    .map(|params| {
                        let collision = !params.is_empty()
                            && ctors.iter().filter(|p| p.len() == params.len()).count() > 1;
                        typeck::ctor_link_name(&ty, params, collision)
                    })
                    .collect::<Vec<_>>()
            }
            // 无显式 ctor（默认 ctor）或查不到布局：退化为无参名（旧行为）。
            _ => vec![format!("__ctor::{ty}")],
        };
        for ctor_key in ctor_keys {
            if let Some(&id) = name_to_id.get(&ctor_key) {
                reachable.insert(id);
            }
        }
    }
    // 静态字段初始化器中的函数调用（如 RegisterProperty<string>）
    // 必须保留——这些调用仅在 codegen 生成的 __sinit_ 中出现，
    // MIR 函数体不直接引用它们，导致 reachability 分析将其剪除。
    for sf in &layouts.static_fields {
        if let Some(init_expr) = &sf.init {
            if let Expr::Call {
                func: box_func,
                type_args,
                ..
            } = &init_expr.node
            {
                if let Expr::Ident(func_name) = &box_func.node {
                    let type_ids: Vec<TypeId> = type_args
                        .iter()
                        .map(|t| {
                            if let Type::Named { path, .. } = &t.node {
                                match path.last().map(|i| i.as_str()).unwrap_or("void") {
                                    "int" => TypeId::Int,
                                    "long" => TypeId::Long,
                                    "short" => TypeId::Short,
                                    "byte" => TypeId::Byte,
                                    "uint" => TypeId::UInt,
                                    "ushort" => TypeId::UShort,
                                    "sbyte" => TypeId::SByte,
                                    "char" => TypeId::Char,
                                    "bool" => TypeId::Bool,
                                    "float" => TypeId::Float,
                                    "double" => TypeId::Double,
                                    "string" => TypeId::String,
                                    "object" => TypeId::Object,
                                    "void" => TypeId::Void,
                                    other => TypeId::Named(other.into()),
                                }
                            } else {
                                TypeId::Void
                            }
                        })
                        .collect();
                    let mangled = typeck::mangle_generic(func_name, &type_ids);
                    if let Some(&id) = name_to_id.get(&mangled) {
                        reachable.insert(id);
                    }
                }
            }
        }
    }
    expand_reachable_callees(&mut reachable);

    if std::env::var("ARC_DEBUG_REACH").is_ok() {
        for (n, id) in &name_to_id {
            if n.contains("Register_Base_Impl") || n.contains("Registrar") {
                eprintln!(
                    "[reach-g3a] name={n} id={id} reachable={}",
                    reachable.contains(id)
                );
            }
        }
    }

    let mut out: Vec<(String, mir::MirCfgBody)> = mir_fns
        .into_iter()
        .enumerate()
        .filter(|(i, _)| reachable.contains(&(*i as u32)))
        .map(|(_, pair)| pair)
        .collect();

    // 内置 stub 类**属性 getter**（`<Class>::get_<Prop>`）为 get-only
    // custom-accessor 属性：typeck 不为其合成 MirCfgBody（方法级 stub 类如
    // `SetText` 有显式方法体才进 mir_fns；属性无源体被丢弃）。但 codegen 在
    // 属性访问点按 `mangle_method(Class, get_X)` 直调 `@<Class>_get_X` 符号，
    // MIR 调用图**无 Call 边**——上方 force-keep 只覆盖已在 mir_fns 的 stub
    // 名，抓不到这类未入表 getter。缺失 → stub 无从发射 → `arc-prune-001`
    //（**TextBuffer_get_LineCount 复现**：code 编辑器行号读取 `[Builtin]`
    // 属性 `LineCount`，走 custom-accessor getter 直调，不在 mir_fns → 被
    // 剪除 → 调用体仍 `call` 其符号而 stub 未发射）。此处为 stub-handled 类
    // 的每只可读属性合成 `MirCfgBody::stub_skeleton`，使其进入 fns 发射遍历，
    // 经 `try_emit_stub` 生成 linkonce_odr stub。
    //
    // 安全性：`is_builtin_stub_fn` 与 `try_emit_stub` 判定一致（FileStream
    // 排除的 OpenRead/OpenWrite/Create 同样被 `class_is_stub_handled` 排除），
    // 故合成的条目 `try_emit_stub` 必返回 Some（有效 stub），不会落入空体发射。
    for (class_name, cl) in &layouts.classes {
        for prop in &cl.declared_properties {
            if !prop.can_read {
                continue;
            }
            let link = format!("{class_name}::get_{}", prop.name);
            if !codegen::is_builtin_stub_fn(&link) {
                continue;
            }
            if name_to_id.contains_key(link.as_str()) {
                // 已有真实 MIR body / 已被上方 stub force-keep 保留 → 不重复。
                continue;
            }
            out.push((
                link.clone(),
                mir::MirCfgBody::stub_skeleton(&link, class_name),
            ));
        }
    }

    out
}

/// 从单态字典类名提取用户类型键后缀（需 trampoline 的 K）。
/// 基元/`string` 键走 runtime 内置 hash/eq，返回 `None`。
fn dict_user_key_type(class_name: &str) -> Option<&str> {
    let rest = class_name
        .strip_prefix("Dictionary_")
        .or_else(|| class_name.strip_prefix("ConcurrentDictionary_"))?;
    // 与 codegen `KNOWN_TYPE_SUFFIXES` + `string` 对齐：这些键无用户 trampoline。
    const BUILTIN_KEYS: &[&str] = &[
        "int", "long", "short", "byte", "char", "float", "double", "bool", "string", "void",
        "uint", "ulong", "ushort", "sbyte",
    ];
    for k in BUILTIN_KEYS {
        if rest.starts_with(&format!("{k}_")) {
            return None;
        }
    }
    let k = rest.split('_').next()?;
    if k.is_empty() {
        None
    } else {
        Some(k)
    }
}

pub fn emit_parse_error(source: &str, path: &Path, err: &parse::ParseError) {
    let mut files = SimpleFiles::new();
    let file_id = files.add(path.display().to_string(), source);
    let span = match err {
        parse::ParseError::Unexpected { span, .. } => *span,
        parse::ParseError::Eof => ast::Span::DUMMY,
    };
    let diag = Diagnostic::error()
        .with_message(format!("{err}"))
        .with_labels(vec![Label::primary(
            file_id,
            span.start as usize..span.end as usize,
        )]);
    let mut writer = StandardStream::stderr(ColorChoice::Auto);
    let _ = term::emit(&mut writer, &term::Config::default(), &files, &diag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_guard_passthrough_on_success() {
        let v = phase_guard("test", || 42).unwrap();
        assert_eq!(v, 42);
    }

    #[test]
    fn phase_guard_converts_panic_to_diagnostic() {
        let err = phase_guard("mir lower", || -> i32 {
            panic!("boom: unresolved ident P");
        })
        .unwrap_err();
        assert!(
            err.contains("internal compiler error during mir lower"),
            "got: {err}"
        );
        assert!(err.contains("unresolved ident P"), "got: {err}");
    }

    #[test]
    fn phase_guard_propagates_string_panic_payload() {
        let err = phase_guard("typeck", || -> i32 {
            std::panic::panic_any(String::from("custom payload"));
        })
        .unwrap_err();
        assert!(err.contains("custom payload"), "got: {err}");
    }

    // ── QIF-6 过滤表达式引擎测试 ──

    fn make_method(
        class: &str,
        method: &str,
        attr: &str,
        traits: Vec<(&str, &str)>,
    ) -> QifTestMethod {
        make_method_ns("", class, method, attr, traits)
    }

    fn make_method_ns(
        ns: &str,
        class: &str,
        method: &str,
        attr: &str,
        traits: Vec<(&str, &str)>,
    ) -> QifTestMethod {
        QifTestMethod {
            class_name: class.to_string(),
            method_name: method.to_string(),
            attr_name: attr.to_string(),
            inline_data: Vec::new(),
            order: 0,
            display_name: String::new(),
            ctor_param_types: Vec::new(),
            is_async: false,
            collection_name: None,
            skip_reason: None,
            traits: traits
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            namespace: ns.to_string(),
        }
    }

    fn haystacks<'a>(m: &'a QifTestMethod) -> impl Fn(&str) -> Vec<String> + 'a {
        move |field: &str| field_values(m, field)
    }

    #[test]
    fn qif_filter_empty_is_none() {
        let expr = QifFilterExpr::parse("").unwrap();
        assert!(expr.is_none());
    }

    #[test]
    fn qif_filter_short_name_defaults_to_fqname_contains() {
        let expr = QifFilterExpr::parse("ListTests").unwrap().unwrap();
        let m = make_method("ListTests", "Add", "Fact", vec![]);
        assert!(expr.matches(&haystacks(&m)));
        let m2 = make_method("DictTests", "Add", "Fact", vec![]);
        assert!(!expr.matches(&haystacks(&m2)));
    }

    #[test]
    fn qif_filter_field_contains() {
        let expr = QifFilterExpr::parse("ClassName~List").unwrap().unwrap();
        let m = make_method("MyListTests", "Add", "Fact", vec![]);
        assert!(expr.matches(&haystacks(&m)));
        let m2 = make_method("DictTests", "Add", "Fact", vec![]);
        assert!(!expr.matches(&haystacks(&m2)));
    }

    #[test]
    fn qif_filter_field_not_contains() {
        let expr = QifFilterExpr::parse("ClassName~!Skip").unwrap().unwrap();
        let m = make_method("ListTests", "Add", "Fact", vec![]);
        assert!(expr.matches(&haystacks(&m)));
        let m2 = make_method("SkipListTests", "Add", "Fact", vec![]);
        assert!(!expr.matches(&haystacks(&m2)));
    }

    #[test]
    fn qif_filter_field_eq() {
        let expr = QifFilterExpr::parse("Kind=Theory").unwrap().unwrap();
        let m = make_method("ListTests", "Add", "Theory", vec![]);
        assert!(expr.matches(&haystacks(&m)));
        let m2 = make_method("ListTests", "Add", "Fact", vec![]);
        assert!(!expr.matches(&haystacks(&m2)));
    }

    #[test]
    fn qif_filter_or() {
        let expr = QifFilterExpr::parse("ListTests,DictTests")
            .unwrap()
            .unwrap();
        let m1 = make_method("ListTests", "Add", "Fact", vec![]);
        let m2 = make_method("DictTests", "Add", "Fact", vec![]);
        let m3 = make_method("SetTests", "Add", "Fact", vec![]);
        assert!(expr.matches(&haystacks(&m1)));
        assert!(expr.matches(&haystacks(&m2)));
        assert!(!expr.matches(&haystacks(&m3)));
    }

    #[test]
    fn qif_filter_or_pipe() {
        let expr = QifFilterExpr::parse("ListTests|DictTests")
            .unwrap()
            .unwrap();
        let m1 = make_method("ListTests", "Add", "Fact", vec![]);
        let m3 = make_method("SetTests", "Add", "Fact", vec![]);
        assert!(expr.matches(&haystacks(&m1)));
        assert!(!expr.matches(&haystacks(&m3)));
    }

    #[test]
    fn qif_filter_and() {
        let expr = QifFilterExpr::parse("ListTests&Kind=Fact")
            .unwrap()
            .unwrap();
        let m1 = make_method("ListTests", "Add", "Fact", vec![]);
        let m2 = make_method("ListTests", "Add", "Theory", vec![]);
        let m3 = make_method("DictTests", "Add", "Fact", vec![]);
        assert!(expr.matches(&haystacks(&m1)));
        assert!(
            !expr.matches(&haystacks(&m2)),
            "ListTests + Theory should be rejected"
        );
        assert!(
            !expr.matches(&haystacks(&m3)),
            "DictTests + Fact should be rejected"
        );
    }

    #[test]
    fn qif_filter_not() {
        let expr = QifFilterExpr::parse("!ListTests").unwrap().unwrap();
        let m1 = make_method("ListTests", "Add", "Fact", vec![]);
        let m2 = make_method("DictTests", "Add", "Fact", vec![]);
        assert!(!expr.matches(&haystacks(&m1)));
        assert!(expr.matches(&haystacks(&m2)));
    }

    #[test]
    fn qif_filter_trait_contains() {
        let expr = QifFilterExpr::parse("Trait~category:unit")
            .unwrap()
            .unwrap();
        let m1 = make_method("ListTests", "Add", "Fact", vec![("category", "unit")]);
        let m2 = make_method(
            "ListTests",
            "Add",
            "Fact",
            vec![("category", "integration")],
        );
        assert!(expr.matches(&haystacks(&m1)));
        assert!(!expr.matches(&haystacks(&m2)));
    }

    #[test]
    fn qif_filter_complex_parens_and_or() {
        // (A & B) | C
        let expr = QifFilterExpr::parse("(ClassName~List&Kind=Fact)|DictTests")
            .unwrap()
            .unwrap();
        let m1 = make_method("ListTests", "Add", "Fact", vec![]);
        let m2 = make_method("ListTests", "Add", "Theory", vec![]);
        let m3 = make_method("DictTests", "Add", "Fact", vec![]);
        let m4 = make_method("SetTests", "Add", "Fact", vec![]);
        assert!(
            expr.matches(&haystacks(&m1)),
            "ListTests + Fact should pass"
        );
        assert!(
            !expr.matches(&haystacks(&m2)),
            "ListTests + Theory should fail both branches"
        );
        assert!(
            expr.matches(&haystacks(&m3)),
            "DictTests should pass via OR"
        );
        assert!(!expr.matches(&haystacks(&m4)), "SetTests should fail");
    }

    #[test]
    fn qif_filter_apply_empty_passthrough() {
        let methods = vec![
            make_method("ListTests", "Add", "Fact", vec![]),
            make_method("DictTests", "Add", "Fact", vec![]),
        ];
        let got = apply_qif_filter(methods.clone(), "", "", "").unwrap();
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn qif_filter_apply_namespace_and_kind() {
        let methods = vec![
            make_method_ns("Arc.Collections", "ListTests", "Add", "Fact", vec![]),
            make_method_ns("Arc.Collections", "DictTests", "Add", "Theory", vec![]),
            make_method_ns("Arc.Math", "VectorTests", "Norm", "Fact", vec![]),
        ];
        let got = apply_qif_filter(methods.clone(), "", "Arc.Collections", "").unwrap();
        assert_eq!(got.len(), 2);

        let got = apply_qif_filter(methods.clone(), "", "", "Theory").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].class_name, "DictTests");

        let got = apply_qif_filter(methods, "", "Arc.Collections", "Theory").unwrap();
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn qif_filter_invalid_syntax_returns_err() {
        let err = QifFilterExpr::parse("ClassName=").unwrap_err();
        assert!(err.contains("empty value"), "got: {err}");
    }
}
