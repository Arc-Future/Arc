//! RFC 017 M2 gap ①：`Assembly.Entry<T>` 强类型间接调用（codegen）。
//!
//! 纯 IR 单元测试（不依赖 mid-edit 的 `arc` crate），断言：
//! 1. 库侧（EmitRole::DynamicLibrary）：顶层 `Entry() -> Foo`（class）导出
//!    `__arc_entry__{TR_id}_{TR_sig}` 统一 `void*→void*` C ABI wrapper
//!    （指纹段见 `entry_layout_signature`：Foo 无字段无父链 → FNV-64("Foo")）。
//! 2. 调用点（EmitRole::MainObject）：`Assembly.Entry<Foo>()` 降级为
//!    `rt_library_sym(handle, symbol)` + 裸函数指针间接调用 `call ptr %fn(ptr null)`。

use ast::{
    CallingConv, LoadStrategy, NativeFn, NativeModule, NativeParam, ParamDirection, Type, TypeId,
};
use codegen::{compile_module_to_object, EmitRole, GenerateToTable};
use indexmap::IndexMap;
use mir::{
    BlockId, Linkage, LocalId, MirCfgBody, MirOperand, MirRvalue, MirStatement, MirTerminator,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use typeck::{ClassLayout, ProgramLayouts};

/// FNV-1a-32 与 `type_name_to_id` 严格一致（确定性类型身份哈希）。
fn fnv1a_32(s: &str) -> i32 {
    let mut hash: u32 = 2166136261;
    for b in s.bytes() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    if hash == 0 {
        1
    } else {
        hash as i32
    }
}

/// FNV-1a-64 与 `entry_layout_signature` 的叶子路径一致：无布局信息类型
/// （如本测试的无字段 `Foo`）指纹即类型名哈希。
fn fnv1a_64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// 含 `Foo` class 的最小布局（`Entry` 返回 class → wrapper 走 ptr 透传路径）。
fn layouts_with_foo() -> ProgramLayouts {
    let mut classes: IndexMap<ast::Ident, ClassLayout> = IndexMap::new();
    classes.insert(
        "Foo".into(),
        ClassLayout {
            name: "Foo".into(),
            fields: vec![],
            parent: None,
            interfaces: vec![],
            method_impl: IndexMap::new(),
            virtual_slots: vec![],
            has_vtable: false,
            constructors: vec![vec![]],
            declared_methods: vec![],
            declared_properties: vec![],
        },
    );
    ProgramLayouts {
        classes,
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

/// 构造单块 MIR body：给定返回类型、形参、语句列表与终结器。
fn body(
    ret: TypeId,
    params: Vec<(&str, TypeId)>,
    locals: IndexMap<LocalId, (ast::Ident, TypeId)>,
    statements: Vec<MirStatement>,
    terminator: MirTerminator,
) -> MirCfgBody {
    let entry = BlockId(0);
    let mut blocks = IndexMap::new();
    blocks.insert(
        entry,
        mir::MirBlock {
            id: entry,
            statements,
            terminator,
        },
    );
    let param_count = params.len();
    MirCfgBody {
        params: params.into_iter().map(|(n, t)| (n.into(), t)).collect(),
        ret,
        param_count,
        locals,
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
        loop_backedges: HashSet::new(),
        foreach_loops: Vec::new(),
        spill_set: typeck::SpillSet::empty(),
    }
}

/// `rt_library_sym` 的 `.ani` 契约（供 emit_native_decls 发射 `declare`）。
fn rt_library_module() -> NativeModule {
    NativeModule {
        name: "rt_library".into(),
        functions: vec![NativeFn {
            name: "rt_library_sym".into(),
            symbol: None,
            params: vec![
                NativeParam {
                    name: "handle".into(),
                    ty: Type::named("NativePtr"),
                    direction: ParamDirection::In,
                },
                NativeParam {
                    name: "name".into(),
                    ty: Type::named("string"),
                    direction: ParamDirection::In,
                },
            ],
            ret: Some(Type::named("NativePtr")),
            calling_conv: CallingConv::C,
        }],
        types: vec![],
        capability: None,
        callbacks: vec![],
        library: None,
        library_env_var: None,
        source: None,
        load: LoadStrategy::Static,
    }
}

/// 编译 MIR → 读回 `.ll` 文本。
fn compile_to_ll(
    fns: &[(String, MirCfgBody)],
    layouts: &ProgramLayouts,
    emit_role: EmitRole,
    stem: &str,
) -> String {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/codegen_assembly_entry")
        .join(stem);
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    let obj = out_dir.join("out.o");
    let _diags = compile_module_to_object(
        fns,
        layouts,
        &obj,
        Some(&out_dir),
        None,
        false,
        "assembly_entry.as",
        "",
        false,
        &HashMap::new(),
        &[rt_library_module()],
        &GenerateToTable::default(),
        &[],
        emit_role,
        None,
        // RFC 017 产物域：本测试读回 IR 文本，显式保留 out.ll。
        true,
    )
    .expect("compile_module_to_object");
    let ll = out_dir.join("out").join("out.ll");
    std::fs::read_to_string(&ll).unwrap_or_else(|e| panic!("read {}: {e}", ll.display()))
}

/// ①-1：库侧（DynamicLibrary）导出 `__arc_entry__{TR_id}_{TR_sig}` wrapper（class 返回）。
#[test]
fn entry_wrapper_symbol_is_emitted_for_class_library() {
    let entry_body = body(
        TypeId::Named("Foo".into()),
        vec![],
        IndexMap::new(),
        vec![],
        MirTerminator::Return(Some(MirOperand::ConstNull)),
    );
    let fns = vec![("Entry".to_string(), entry_body)];
    let ir = compile_to_ll(
        &fns,
        &layouts_with_foo(),
        EmitRole::DynamicLibrary,
        "wrapper",
    );

    let tr_id = fnv1a_32("Foo");
    let tr_sig = fnv1a_64("Foo");
    let symbol = format!("__arc_entry__{tr_id}_{tr_sig}");
    assert!(
        ir.contains(&format!("define ptr @{symbol}(ptr %unused)")),
        "expected Entry wrapper `{symbol}`, got IR:\n{ir}"
    );
}

/// ①-2：调用点（MainObject）`Assembly.Entry<Foo>()` → rt_library_sym + 间接调用。
#[test]
fn assembly_entry_call_site_emits_typed_indirect_call() {
    let mut locals = IndexMap::new();
    locals.insert(LocalId(0), ("asm".into(), TypeId::Named("Assembly".into())));
    locals.insert(
        LocalId(1),
        (
            "result".into(),
            TypeId::Nullable {
                inner: Box::new(TypeId::Named("Foo".into())),
            },
        ),
    );

    let host_body = body(
        TypeId::Void,
        vec![],
        locals,
        vec![MirStatement::Assign {
            place: LocalId(1),
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(LocalId(0)),
                method: "Entry".into(),
                args: vec![],
                receiver_type: "Assembly".into(),
                impl_class: Some("Assembly".into()),
                target_fn: Some("Assembly::Entry__Foo".into()),
                is_virtual: false,
                params: vec![],
            },
        }],
        MirTerminator::Return(None),
    );

    // 缺失分支 throw 需 `__ctor::EntryPointNotFoundException_1` 符号（空体占位）。
    // 形参须同时注册进 locals（codegen 按 locals 发射 alloca，再按 params 存值）。
    let mut ctor_locals = IndexMap::new();
    ctor_locals.insert(LocalId(0), ("self".into(), TypeId::Object));
    ctor_locals.insert(LocalId(1), ("msg".into(), TypeId::String));
    let ctor_body = body(
        TypeId::Void,
        vec![("self", TypeId::Object), ("msg", TypeId::String)],
        ctor_locals,
        vec![],
        MirTerminator::Return(None),
    );

    let fns = vec![
        ("Host".to_string(), host_body),
        (
            "__ctor::EntryPointNotFoundException_1".to_string(),
            ctor_body,
        ),
    ];
    let ir = compile_to_ll(&fns, &layouts_with_foo(), EmitRole::MainObject, "call_site");

    let tr_id = fnv1a_32("Foo");
    let tr_sig = fnv1a_64("Foo");
    let symbol = format!("__arc_entry__{tr_id}_{tr_sig}");
    // 符号名内联进字符串常量（FNV-1a-32 类型身份 + FNV-1a-64 布局指纹）。
    assert!(
        ir.contains(&format!("c\"{symbol}\\00\"")),
        "expected symbol constant `{symbol}`, got IR:\n{ir}"
    );
    // rt_library_sym 解析函数指针。
    assert!(
        ir.contains("call ptr @rt_library_sym"),
        "expected rt_library_sym call, got IR:\n{ir}"
    );
    // 裸函数指针间接调用（void*→void*：无参入参槽传 null）。
    assert!(
        ir.contains("= call ptr %"),
        "expected indirect `call ptr %fn`, got IR:\n{ir}"
    );
}
