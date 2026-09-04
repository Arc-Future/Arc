//! LLVM 22 native code generation for Arc.
//!
//! Generates LLVM IR text from MIR, then invokes clang for AOT compilation.
//! This is the sole backend — Arc is a native LLVM language, not a C translator.

pub mod arcdbg;
mod compile;
pub mod docgen;
mod emit_role;
mod error;
mod generate_to_table;
mod llvm_ir;
pub mod resx_compiler;
/// runtime `.o` 缓存内容寻址（命中判定 = 源码/依赖树/选项内容哈希，弃 mtime）。
pub mod rt_cache;
pub mod sdk_layout;

pub use compile::{
    compile_module, compile_module_to_dynamic_library, compile_module_to_object,
    link_objects_to_dynamic_library, link_objects_to_executable, PackageMeta, ProjectKind,
};
pub use emit_role::EmitRole;
pub use error::CodegenError;
pub use generate_to_table::{GenerateToEntry, GenerateToTable};
/// clang 二进制解析（单一解析序；`arc env` / `arc doctor` 复用）。
pub use llvm_ir::mangle::clang_path;
/// 静态初始化依赖分析的结构化编译期诊断（`arc-sinit-001/002`），由 arc CLI
/// pipeline 统一渲染为 `warning[<code>]: <message>`（对齐 `arc-cycle-001` 通道）。
pub use llvm_ir::static_init_diag::StaticInitDiagnostic;

/// 判断函数名是否由 builtin 集合 stub（List/Dictionary/Weak 等）处理。
///
/// 供 `arc` pipeline 在泛型模板剔除时保留 stub-handled 名：stub IR 由
/// `emit_stubs` 直接生成（不依赖 MIR body），且可能被发射的单态化实例
/// 引用（如 `AssemblyLoadContext` 调用 `Weak_T_GetWeakSlot`）——一并剔除
/// 会致 stub 缺失 → `--dynamic` 库 `undefined value @Weak_T_*`。
pub fn is_builtin_stub_fn(name: &str) -> bool {
    llvm_ir::emit_stubs::class_is_stub_handled(name)
}
