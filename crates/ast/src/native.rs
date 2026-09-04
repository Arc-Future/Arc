//! 跨语言互操作契约 AST（RFC 016 M1/M2/M3）。
//!
//! 定义 `.ani` 契约文件的语法树节点。M1 支持基元类型与 `string`/`string?`；
//! M2 扩展 `out`/`ref` 参数方向（`ParamDirection`）。
//! M3 扩展（RFC 016 §3.1/§3.3）：
//! - `NativeTypeDecl`/`NativeTypeKind`：契约 `native type` 声明（`OpaquePtr`/`Struct`）
//! - `CallingConv`：调用约定（`C`/`Stdcall`），通过 `stdcall` 修饰符指定
//! - `NativeModule.types`：契约类型声明列表
//! - `NativeModule.capability`：能力 gating 标签（Phase 0 仅记录，Phase 1+ 强制）
//! - `NativeFn.calling_conv`：函数级调用约定
//! - `NativeModule.load`：运行时库加载统一模型（RFC 016 `load` 声明）
//! - `NativeModule.library` 两形态（用户裁决简化：单一 `.ani` 协议，无多层路径托底）：
//!   字面量相对路径（相对执行程序根目录）或 `Environment.GetEnvironmentVariable(...)`
//!
//! 能力 gating 强制由 [4.4] 落地后启用。

use crate::{Ident, Spanned, Type};
use std::path::PathBuf;

/// 一个 `.ani` 文件解析为一个 `NativeModule`，描述单个外部 C 库的公开符号。
///
/// `name` 同时映射到链接库名（Linux/macOS: `-l<name>`；Windows MSVC: `<name>.lib`）。
#[derive(Clone, Debug, PartialEq)]
pub struct NativeModule {
    pub name: Ident,
    pub functions: Vec<NativeFn>,
    /// RFC 016 M3：契约类型声明（`native type`）。
    pub types: Vec<NativeTypeDecl>,
    /// RFC 016 M3：能力 gating 标签。`None` 表示无能力要求（Phase 0 仅记录）。
    pub capability: Option<Ident>,
    /// RFC 016 M1：native callback 类型声明。
    pub callbacks: Vec<NativeCallback>,
    /// RFC 016 M4（用户裁决简化 2026-08-03）：per-module 库路径（单一 `.ani` 协议，
    /// 无多层路径托底）。
    ///
    /// 由契约内 `library = "..."` 字面量声明。**相对路径基准 = 执行程序根目录**
    /// （codegen 按 `-o` 输出可执行文件所在目录解析为绝对路径；编译期烘焙）。
    /// 解析该模块的库文件时此目录优先级最高，其次才是 `ani-native-lib` 搜索列表 /
    /// vendor 注入 / 系统路径。`None` 表示不声明，仅使用全局搜索列表。
    ///
    /// 与 [`NativeModule::library_env_var`] 为**二选一**（单一惯用法，单一 `library`
    /// 声明）：字面量路径供 static/auto 编译期搜索链与 runtime 运行时第一候选；
    /// 环境变量形式仅参与运行时解析（typeck 强制 `load != Static`）。
    pub library: Option<PathBuf>,
    /// RFC 016（2026-08-03 扩展，用户裁决简化）：`library` 的**环境变量形式**——
    /// `library = Environment.GetEnvironmentVariable("NAME");`。
    ///
    /// 运行时懒解析器在 `rt_library_load` 前调用 `rt_env_get_var("NAME")` 求值
    /// 得到库**目录**（与字面量形式同语义，平台库名由编译期烘焙追加）；空串/
    /// 未设置 → 该候选缺失，优雅降级（`Native.IsAvailable=false` / 调用抛
    /// `NativeLibraryNotFoundException`）。返回相对路径同样按执行程序根目录解析。
    /// `None` 表示 `library` 未用环境变量形式声明。
    pub library_env_var: Option<String>,
    /// RFC 016（native 源实现增补）：模块的 **C 源实现**路径。
    ///
    /// 与 `library`（链接**已编译**外部库/DLL）平行，`source` 声明一段**随项目
    /// 编译纳入**的 C 源码：声明 `source = "foo.c";` 后，编译器经该项**发现**该 C
    /// 源 → 用 clang 编译为 `.o` → 与其余对象一并链接进产物，并跳过该模块的外部
    /// `-l<name>` 与外部库符号验证（符号由本地编译的 `.o` 提供）。
    ///
    /// **路径基准 = 该 `.ani` 契约文件所在目录**（相对 .ani 的源码路径）；由加载器
    /// `scan_contract_dir` 在解析后解析为绝对路径。`None` 表示模块来自外部库/DLL
    ///（经 `library`/搜索列表链接），不使用 C 源实现接缝。
    ///
    /// 对齐 DLL 显式声明模型：`library` 声明库路径、`source` 声明源码路径，单一
    /// `.ani` 协议内二选一——真实用户引入原生能力无需改动编译器（编译器只认
    /// `.ani` 契约，C 源路径由契约自声明）。
    pub source: Option<PathBuf>,
    /// RFC 016：模块加载策略（`load = "static" | "runtime" | "auto"`）。
    ///
    /// - `Static`（默认）：编译期符号验证 + 静态链接（与 RFC 016 现状一致）。
    /// - `Runtime`：codegen 生成懒解析器，运行时候选路径依次 `rt_library_load`，
    ///   成功后逐符号 `rt_library_sym` 填充 per-module 函数表，调用经间接跳转。
    /// - `Auto`：编译期可定位（verify_symbols 成功）则 static，否则降级 runtime。
    pub load: LoadStrategy,
}

/// RFC 016：`.ani` 模块加载策略（`load` 统一模型）。
///
/// 单一惯用法——无双轨 API；`static` 为零行为变更基线，`runtime`/`auto` 提供
/// 「运行时计算库路径」能力（检测机器是否安装某组件后决定加载哪目录的库）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoadStrategy {
    /// 默认：编译期符号验证 + 静态链接（等同 RFC 016 现状，零行为变更）。
    #[default]
    Static,
    /// 运行时加载：codegen 生成懒解析器 + per-module 函数表 + 间接调用。
    Runtime,
    /// 自动分流：编译期可定位（`verify_symbols` 成功）则 static，否则降级 runtime。
    Auto,
}

/// 契约函数声明。无函数体——仅签名供 typeck 校验与 codegen 生成 `declare`。
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFn {
    /// Arc 侧函数名，调用方通过 `Module.Name(...)` 访问。
    pub name: Ident,
    /// 显式 C 符号名覆盖；`None` 时默认使用 `name`。
    ///
    /// 用于 Arc 风格 PascalCase 与 C 风格 snake_case 不一致的场景，
    /// 例如 `name = "Puts"`、`symbol = Some("puts")`。
    pub symbol: Option<Ident>,
    pub params: Vec<NativeParam>,
    pub ret: Option<Spanned<Type>>,
    /// RFC 016 M3：调用约定。默认 `C`；`stdcall` 修饰符指定 `Stdcall`。
    pub calling_conv: CallingConv,
}

/// 调用约定（RFC 016 M3）。
///
/// - `C`：默认 C 调用约定（LLVM IR `ccc`，可省略前缀）
/// - `Stdcall`：Windows stdcall（LLVM IR `stdcallcc`）；非 Windows 平台降级为 `C`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CallingConv {
    #[default]
    C,
    Stdcall,
}

/// 契约类型声明（RFC 016 M3 §3.1）。
///
/// 由 `.ani` 中 `native type Name;` 或 `native type Name { ... };` 声明。
#[derive(Clone, Debug, PartialEq)]
pub struct NativeTypeDecl {
    pub name: Ident,
    pub kind: NativeTypeKind,
}

/// 契约类型种类（RFC 016 M3 §3.3）。
///
/// - `OpaquePtr`：不透明指针——`native type Name;` 声明，对应 C `void*`，
///   按 `ptr` 传递。`NativePtr` 是内置的 `OpaquePtr` 类型，无需声明即可使用。
/// - `Struct`：契约 struct——`native type Name { T1 f1; T2 f2; };` 声明，
///   对应 C `struct Name { ... }`，按值传递（LLVM `%struct.Name`）。
#[derive(Clone, Debug, PartialEq)]
pub enum NativeTypeKind {
    OpaquePtr,
    Struct { fields: Vec<(Ident, Spanned<Type>)> },
}

/// 契约参数方向（RFC 016 M2）。
///
/// - `In`：默认值传递（C 端只读）
/// - `Out`：C# `out` 语义——codegen 生成栈临时变量 + out-pointer，调用后回写 Arc 变量
/// - `InOut`：C# `ref` 语义——传入地址，C 函数可读可写
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ParamDirection {
    #[default]
    In,
    Out,
    InOut,
}

/// 契约参数。M2 起支持 `out`/`ref` 方向（`ParamDirection`）。
#[derive(Clone, Debug, PartialEq)]
pub struct NativeParam {
    pub name: Ident,
    pub ty: Spanned<Type>,
    #[doc = "参数方向（RFC 016 M2）：`In` 默认；`Out` 对应 `out` 修饰符；`InOut` 对应 `ref`。"]
    pub direction: ParamDirection,
}

/// RFC 016 M1：`.ani` 文件中的 native callback 声明。
///
/// 定义 C 函数指针类型签名，供 Arc 无捕获 lambda 透传为 C 函数指针。
/// 参数语法与 `extern fn` 一致（RFC 016 §3.3 类型白名单约束）。
///
/// 示例：`native callback CmpFn(NativePtr a, NativePtr b) -> int;`
#[derive(Clone, Debug, PartialEq)]
pub struct NativeCallback {
    /// 回调类型名（如 "CmpFn"），在 Arc 侧表现为函数指针类型。
    pub name: Ident,
    /// 回调参数列表。
    pub params: Vec<NativeParam>,
    /// 回调返回类型；`None` 表示 `void`。
    pub ret: Option<Spanned<Type>>,
    /// 调用约定（默认 C）。
    pub calling_conv: CallingConv,
}
