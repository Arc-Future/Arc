//! L1 IR 白盒：接口↔object 表示转换（idx25 家族，2026-09-05 修复）。
//!
//! 复现 shape（chord corpus idx25 `Contribute_RegistryRoutesAndAutoReverts`
//! VEH 取证）：泛型 mono 克隆体（模板 lowering 期 T 未知）——
//! ① `PutT<T=IRegistry>(v)` 体内把 T 形参（接口 fat 盒）透传入 `object?`
//!    形参 → 盒被当对象存入 object 槽，槽位 ARC inc/dec 原子写盒内存 →
//!    obj 半损坏 → 接口分派 0xC0000005；
//! ② `GetT<T=IRegistry>()` 的 `(T)value` cast（object→接口）缺 MakeIfaceDyn
//!    组装 → 返回裸对象，调用方按 `{obj,itable}` 胖盒解引用 → 0xC0000005。
//! 修复（同一变更集，详见 CHANGELOG 9/5）：
//! ① MIR 实参物化环（typed）与 mono 克隆修复（`repair_clone_iface_arg_shapes`）
//!    补 `UnboxIface` 取 obj 半（codegen 为 null 安全 phi：null 盒 → null）；
//! ② codegen `UnboxGeneric` 接口具体目标 → `emit_make_iface_dyn`
//!    （`rt_obj_to_iface` 动态适配，与 typed 路径同构）。
//! 白盒断言：GetT mono 体含 `rt_obj_to_iface` 调用；PutT mono 体含
//! null 安全拆盒 phi 且直传形参盒给 Store_Put 的裸透传不再出现。

use arc_tests::{clang_available, compile_in_process_keep_ir, workspace_root};

fn ir_path(name: &str) -> std::path::PathBuf {
    workspace_root().join(format!("target/arc-tests/{name}/obj/Debug/main/out.ll"))
}

/// 提取 `define ... @<link_name>(` 定义体的文本（到下一个顶层 `define`/`$` 段为止）。
fn fn_body<'a>(ir: &'a str, link_name: &str) -> &'a str {
    let needle = format!("@{link_name}(");
    let mut from = 0;
    let start = loop {
        let Some(pos) = ir[from..].find(&needle) else {
            panic!("expected define for {link_name} in IR");
        };
        let abs = from + pos;
        // 定义行的锚：行首为 `define`（形如 `define linkonce_odr ptr @Name(`）。
        let line_start = ir[..abs].rfind('\n').map(|i| i + 1).unwrap_or(0);
        if ir[line_start..abs].trim_start().starts_with("define ") {
            break line_start;
        }
        from = abs + needle.len();
    };
    let rest = &ir[start..];
    let end = rest
        .find("\ndefine ")
        .or_else(|| rest.find("\n$"))
        .unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn iface_object_conversions_in_mono_bodies() {
    let name = "iface_object_mono_ir";
    if !clang_available() {
        eprintln!("skip {name}: clang not found");
        return;
    }
    compile_in_process_keep_ir(
        name,
        r#"using Arc;

public interface IRegistry {
    int Add(string name);
}

public class Registry : IRegistry {
    private int _count;

    public Registry() {
        _count = 0;
    }

    public int Add(string name) {
        _count = _count + 1;
        return _count;
    }
}

public class Store {
    private object? _slot;

    public Store() {
        _slot = null;
    }

    public void Put(object? value) {
        _slot = value;
    }

    public void PutT<T>(T value) where T : class {
        this.Put(value);
    }

    public T? GetT<T>() where T : class {
        object? value = _slot;
        return value != null ? (T)value : null;
    }
}

public class Program {
    public static int Main() {
        Store store = new Store();
        store.PutT<IRegistry>(new Registry());
        IRegistry? registry = store.GetT<IRegistry>();
        if (registry != null) {
            return registry.Add("x");
        }
        return -1;
    }
}
"#,
        &[],
    )
    .expect("compile ok");
    let ir = std::fs::read_to_string(ir_path(name))
        .unwrap_or_else(|e| panic!("expected LLVM IR at {}: {e}", ir_path(name).display()));

    // ② GetT<T=IRegistry> mono：object→接口须经 rt_obj_to_iface 动态组装
    //（修复前为 UnboxGeneric 引用类型透传——裸对象当胖盒，调用方解引用 AV）。
    let get = fn_body(&ir, "Store_GetT__IRegistry");
    assert!(
        get.contains("rt_obj_to_iface"),
        "expected dynamic iface assembly in Store_GetT__IRegistry, body:\n{get}"
    );

    // ① PutT<T=IRegistry> mono：接口盒入 object? 形参前须 null 安全取 obj 半
    //（修复前形参盒直传 Store_Put——object 槽 ARC 写坏盒内存）。
    let put = fn_body(&ir, "Store_PutT__IRegistry");
    assert!(
        put.contains("Store_Put("),
        "expected call to Store_Put in Store_PutT__IRegistry, body:\n{put}"
    );
    assert!(
        put.contains("icmp eq ptr") && put.contains("phi ptr [ null,"),
        "expected null-safe unbox (icmp + phi) of the iface box in Store_PutT__IRegistry, body:\n{put}"
    );
    // 直传形参盒（arg1 未拆盒即调用）不得再出现：调用实参必须来自拆盒 phi。
    let boxed_direct = "@Store_Put(ptr %t0, ptr %t1)";
    assert!(
        !put.contains(boxed_direct),
        "iface box must not be passed straight into object? param, body:\n{put}"
    );
}
