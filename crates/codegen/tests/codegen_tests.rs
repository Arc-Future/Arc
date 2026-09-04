use codegen::{compile_module, GenerateToTable, ProjectKind};
use indexmap::IndexMap;
use mir::{BlockId, Linkage, MirCfgBody, MirOperand, MirRvalue, MirStatement, MirTerminator};
use std::collections::HashMap;

#[test]
fn llvm_backend_hello() {
    if !clang_available() {
        eprintln!("skip llvm_backend_hello: clang not found");
        return;
    }
    let entry = BlockId(0);
    let mut blocks = IndexMap::new();
    blocks.insert(
        entry,
        mir::MirBlock {
            id: entry,
            statements: vec![MirStatement::Assign {
                place: mir::LocalId(0),
                rvalue: MirRvalue::Call {
                    func: "Console.WriteLine".into(),
                    args: vec![MirOperand::ConstString("Hello, World!".into())],
                },
            }],
            terminator: MirTerminator::Return(None),
        },
    );
    let body = MirCfgBody {
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
    };
    let fns = vec![("Main".to_string(), body)];
    let layouts = typeck::ProgramLayouts {
        classes: Default::default(),
        structs: Default::default(),
        enums: Default::default(),
        enum_variants: Default::default(),
        interfaces: Default::default(),
        variants: Default::default(),
        static_fields: Default::default(),
        observable_properties: Default::default(),
        type_full_names: Default::default(),
    };
    let out = std::env::temp_dir().join("dc_test_hello.exe");
    let _diags = compile_module(
        &fns,
        &layouts,
        &out,
        None,
        None,
        false,
        "test.as",
        "",
        false,
        &HashMap::new(),
        &[],
        &[],
        &GenerateToTable::default(),
        &[],
        ProjectKind::Executable,
        // RFC 017 产物域：本测试不读 IR 文本，走默认焚毁（.ll 不落盘）。
        false,
    )
    .expect("compile hello");
}

fn clang_available() -> bool {
    std::process::Command::new("clang")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        || std::path::Path::new(r"C:\Program Files\LLVM\bin\clang.exe").exists()
}
