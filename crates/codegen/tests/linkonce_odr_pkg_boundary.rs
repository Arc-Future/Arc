//! RFC 017 动态库 / 包边界：全局 dbg 表发射（阶段 4 产物收口后角色收敛）。
//!
//! 验收（非 Skip）：
//! 1. `EmitRole::MainObject` → 发射 external `@__arc_dbg_table` / `@__arc_dbg_count`
//! 2. `EmitRole::DynamicLibrary` → 同样发射 dbg 表（共享库自含 runtime 就地解析）

use codegen::{compile_module_to_object, EmitRole, GenerateToTable};
use indexmap::IndexMap;
use mir::{BlockId, Linkage, MirCfgBody, MirTerminator};
use std::collections::HashMap;
use std::path::PathBuf;

fn empty_main_body() -> MirCfgBody {
    let entry = BlockId(0);
    let mut blocks = IndexMap::new();
    blocks.insert(
        entry,
        mir::MirBlock {
            id: entry,
            statements: vec![],
            terminator: MirTerminator::Return(None),
        },
    );
    MirCfgBody {
        params: vec![],
        ret: ast::TypeId::Void,
        param_count: 0,
        locals: Default::default(),
        entry,
        blocks,
        is_async: false,
        owner: None,
        class_fields: vec![],
        is_ctor: false,
        is_static: false,
        captures: vec![],
        linkage: Linkage::External,
        parallelize: false,
        loop_backedges: std::collections::HashSet::new(),
        foreach_loops: Vec::new(),
        spill_set: typeck::SpillSet::empty(),
    }
}

fn empty_layouts() -> typeck::ProgramLayouts {
    typeck::ProgramLayouts {
        classes: Default::default(),
        structs: Default::default(),
        enums: Default::default(),
        enum_variants: Default::default(),
        interfaces: Default::default(),
        variants: Default::default(),
        static_fields: Default::default(),
        observable_properties: Default::default(),
        type_full_names: Default::default(),
    }
}

fn clang_available() -> bool {
    std::process::Command::new("clang")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        || std::path::Path::new(r"C:\Program Files\LLVM\bin\clang.exe").exists()
}

fn compile_role_to_ll(role: EmitRole, stem: &str) -> String {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/codegen_pkg_boundary")
        .join(stem);
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    let obj = out_dir.join("out.o");
    let fns = vec![("Main".to_string(), empty_main_body())];
    let _diags = compile_module_to_object(
        &fns,
        &empty_layouts(),
        &obj,
        Some(&out_dir),
        None,
        false,
        "pkg_boundary.as",
        "",
        false,
        &HashMap::new(),
        &[],
        &GenerateToTable::default(),
        &[],
        role,
        None,
        // RFC 017 产物域：本测试断言 IR 文本内容，显式保留 out.ll。
        true,
    )
    .expect("compile_module_to_object");
    let ll = out_dir.join("out").join("out.ll");
    std::fs::read_to_string(&ll).unwrap_or_else(|e| {
        panic!("read {}: {e}", ll.display());
    })
}

#[test]
fn dynamic_library_emits_dbg_table() {
    if !clang_available() {
        panic!("clang required for non-Skip package-boundary codegen test");
    }
    let ir = compile_role_to_ll(EmitRole::DynamicLibrary, "dyn_lib_role");
    assert!(
        ir.contains("@__arc_dbg_table = constant"),
        "DynamicLibrary must emit @__arc_dbg_table"
    );
    assert!(
        ir.contains("@__arc_dbg_count = constant i32"),
        "DynamicLibrary must emit @__arc_dbg_count"
    );
}

#[test]
fn main_object_emits_external_dbg_table() {
    if !clang_available() {
        panic!("clang required for non-Skip package-boundary codegen test");
    }
    let ir = compile_role_to_ll(EmitRole::MainObject, "main_role");
    assert!(
        ir.contains("@__arc_dbg_table = constant"),
        "MainObject must emit @__arc_dbg_table"
    );
    assert!(
        ir.contains("@__arc_dbg_count = constant i32"),
        "MainObject must emit @__arc_dbg_count"
    );
    assert!(
        !ir.contains("@__arc_dbg_table = linkonce_odr"),
        "MainObject dbg table must be external strong (not linkonce_odr)"
    );
}
