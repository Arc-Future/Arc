//! 从 arc-integration 迁移的 L1 IR/codegen 白盒测试（a2627a0f 退场承接）。
//!
//! 两类来源：
//! - IR 文本断言：tbaa_metadata / user_type_tbaa / lifetime_markers —— 原经 CLI
//!   `arc build -o out` 读 `obj/Debug/out/out.ll`；本文件改用进程内
//!   `compile_in_process_keep_ir`（与 CLI 同管线：IR 落于 `obj_dir/<output_stem>/out.ll`，
//!   见 codegen/src/llvm_ir/mod.rs 的 work_dir 规则），out 固定为 `main`，故 IR
//!   位于 `target/arc-tests/<name>/obj/Debug/main/out.ll`。默认构建已焚毁 .ll
//!   （RFC 017 产物域），故 IR 断言测试须走保留 IR 变体显式落盘。
//! - 编译失败路径：nll_borrow —— NLL 检查在 prepare_compilation（codegen 前）
//!   执行，直接断言诊断文本（错误码 + P3 友好措辞），无需 clang。
//!
//! 原文件中的运行时断言类（comdat_fqn / mir_operand_discriminant /
//! soa_field_fusion / soa_fusion / reflection_metadata / reflection_typeof /
//! debug_info）归 L2（full-rt 门控），不在本文件；soa_layout 为运行时 C API
//! 白盒（clang 编译 .c 链接 rt），不入 arc-tests 框架。

use arc_tests::{clang_available, compile_in_process, compile_in_process_keep_ir, workspace_root};

/// 进程内编译产物（main）对应的 LLVM IR 文本路径。
fn ir_path(name: &str) -> std::path::PathBuf {
    workspace_root().join(format!("target/arc-tests/{name}/obj/Debug/main/out.ll"))
}

// ── IR 文本白盒（需 clang：codegen 链接） ──

/// RFC 015 M1：`List<T>` 索引器热路径发射 `!tbaa`（data@0 / size@8）。
#[test]
fn ir_list_index_tbaa_metadata() {
    let name = "ir_tbaa_list";
    if !clang_available() {
        eprintln!("skip {name}: clang not found");
        return;
    }
    // RFC 017 产物域：读回 IR 文本 → 显式保留 out.ll。
    compile_in_process_keep_ir(
        name,
        r#"using Arc;
using Arc.Collections;

public class Program {
    public static int Main() {
        List<int> xs = new List<int>();
        xs.Add(10);
        xs.Add(20);
        return xs[0] + xs[1];
    }
}
"#,
        &[],
    )
    .expect("compile ok");
    let ir = std::fs::read_to_string(ir_path(name))
        .unwrap_or_else(|e| panic!("expected LLVM IR at {}: {e}", ir_path(name).display()));
    // M1：RtList struct 类型节点 + data@0/size@8 字段 tag 必须发射。
    assert!(
        ir.contains(r#"!"arc_rt_list""#),
        "expected arc_rt_list TBAA struct node, IR snippet:\n{}",
        &ir[..ir.len().min(2500)]
    );
    assert!(
        ir.contains(", i64 0}") && ir.contains(", i64 8}"),
        "expected RtList field tags (data@0 / size@8), IR snippet:\n{}",
        &ir[..ir.len().min(2500)]
    );
    assert!(
        ir.contains("load ptr") && ir.contains("!tbaa"),
        "expected indexed load with !tbaa metadata, IR snippet:\n{}",
        &ir[..ir.len().min(2500)]
    );
}

/// RFC 015 M5：用户 struct 字段访问发射 struct-path TBAA（X@0 / Y@4）。
#[test]
fn ir_user_type_tbaa_metadata() {
    let name = "ir_user_tbaa";
    if !clang_available() {
        eprintln!("skip {name}: clang not found");
        return;
    }
    // RFC 017 产物域：读回 IR 文本 → 显式保留 out.ll。
    compile_in_process_keep_ir(
        name,
        r#"using Arc;

struct Point {
    public int X;
    public int Y;
    public Point(int x, int y) {
        this.X = x;
        this.Y = y;
    }
    public int Sum() { return this.X + this.Y; }
}

public class Program {
    public static int Main() {
        Point p = new Point(3, 4);
        p.X = 10;
        return p.Sum();
    }
}
"#,
        &[],
    )
    .expect("compile ok");
    let ir = std::fs::read_to_string(ir_path(name))
        .unwrap_or_else(|e| panic!("expected LLVM IR at {}: {e}", ir_path(name).display()));
    // M5：用户 struct 类型节点必须发射（对齐 M1 arc_rt_list 断言）。
    assert!(
        ir.contains(r#"!"Point""#),
        "expected user struct Point TBAA node, IR snippet:\n{}",
        &ir[..ir.len().min(2500)]
    );
    // 字段访问 tag 需覆盖 X@0 / Y@4（同 struct 不同 offset）。
    assert!(
        ir.contains(", i64 0}") && ir.contains(", i64 4}"),
        "expected Point field tags (X@0 / Y@4), IR snippet:\n{}",
        &ir[..ir.len().min(2500)]
    );
    // 至少一条 load 与一条 store 挂上 !tbaa（字段读 + `p.X = 10` 写）。
    assert!(
        ir.contains("load") && ir.contains("store") && ir.contains("!tbaa"),
        "expected load/store with !tbaa metadata, IR snippet:\n{}",
        &ir[..ir.len().min(2500)]
    );
}

/// RFC 015 M2：栈局部 alloca 发射成对的 lifetime.start/end intrinsic 调用。
#[test]
fn ir_lifetime_markers() {
    let name = "ir_lifetime";
    if !clang_available() {
        eprintln!("skip {name}: clang not found");
        return;
    }
    // RFC 017 产物域：读回 IR 文本 → 显式保留 out.ll。
    compile_in_process_keep_ir(
        name,
        r#"using Arc;

public class Program {
    public static int Main() {
        int a = 10;
        int b = 20;
        long c = 5;
        double d = 2.5;
        int sum = a + b + (int)c;
        return sum + (int)d;
    }
}
"#,
        &[],
    )
    .expect("compile ok");
    let ir = std::fs::read_to_string(ir_path(name))
        .unwrap_or_else(|e| panic!("expected LLVM IR at {}: {e}", ir_path(name).display()));
    // M2：同步函数必须同时发射 lifetime.start 与 lifetime.end 调用指令
    //（区别于 declare 声明行，须匹配 `call void @llvm.lifetime...`）。
    assert!(
        ir.contains("call void @llvm.lifetime.start.p0"),
        "expected a llvm.lifetime.start.p0 call, IR snippet:\n{}",
        &ir[..ir.len().min(2500)]
    );
    assert!(
        ir.contains("call void @llvm.lifetime.end.p0"),
        "expected a llvm.lifetime.end.p0 call, IR snippet:\n{}",
        &ir[..ir.len().min(2500)]
    );
    // start 与 end 的调用参数（i64 尺寸 + ptr 槽）必须一致成对。
    let starts: Vec<&str> = ir
        .lines()
        .filter(|l| l.contains("call void @llvm.lifetime.start.p0"))
        .collect();
    let ends: Vec<&str> = ir
        .lines()
        .filter(|l| l.contains("call void @llvm.lifetime.end.p0"))
        .collect();
    assert!(
        !starts.is_empty() && !ends.is_empty(),
        "expected paired start/end markers, starts={} ends={}",
        starts.len(),
        ends.len()
    );
}

// ── NLL 编译失败路径（无需 clang：prepare_compilation 阶段即拒绝） ──

/// P3 约束的通用断言：诊断头允许管线层包装，消息体不得暴露内部术语。
fn assert_nll_p3_wording(err: &str) {
    let body = err
        .strip_prefix("NLL borrow check failed:\n")
        .unwrap_or(err);
    assert!(
        !body.contains("borrow"),
        "error message body must not expose 'borrow' term, got: {err}"
    );
    assert!(
        !body.contains("loan"),
        "error message must not expose 'loan' term, got: {err}"
    );
    assert!(
        !body.contains("lifetime"),
        "error message must not expose 'lifetime' term, got: {err}"
    );
}

/// NLL 检测迭代器失效（`E_ITERATOR_INVALIDATION`）。
#[test]
fn nll_strict_detects_iterator_invalidation() {
    let err = compile_in_process(
        "ir_nll_iter_inv",
        r#"using Arc;
using Arc.Collections;

void Main() {
    var v = new List<int>();
    v.Add(1);
    v.Add(2);
    foreach (var x in v) {
        v.Add(x);
    }
}
"#,
        &[],
    )
    .expect_err("NLL must reject iterator invalidation");
    assert!(
        err.contains("NLL borrow check failed"),
        "expected NLL error, got: {err}"
    );
    assert!(
        err.contains("E_ITERATOR_INVALIDATION"),
        "expected E_ITERATOR_INVALIDATION code, got: {err}"
    );
}

/// P3 约束：错误信息不含「borrow」「loan」「lifetime」术语 + 友好措辞。
#[test]
fn nll_error_message_uses_friendly_wording() {
    let err = compile_in_process(
        "ir_nll_p3_wording",
        r#"using Arc;
using Arc.Collections;

void Main() {
    var v = new List<int>();
    v.Add(1);
    foreach (var x in v) {
        v.Add(x);
    }
}
"#,
        &[],
    )
    .expect_err("NLL must reject iterator invalidation");
    assert_nll_p3_wording(&err);
    assert!(
        err.contains("迭代期间被修改") || err.contains(".ToList()"),
        "error message should contain friendly wording, got: {err}"
    );
}

/// NLL 检测 mutable conflict（`E_BORROW_CONFLICT`）：`Swap(ref x, ref x)`。
#[test]
fn nll_strict_detects_mutable_conflict() {
    let err = compile_in_process(
        "ir_nll_mut_conflict",
        r#"using Arc;

class Helper {
    public void Swap(ref int left, ref int right) {
        int t = left;
        left = right;
        right = t;
    }
}

void Main() {
    var h = new Helper();
    int x = 1;
    h.Swap(ref x, ref x);
}
"#,
        &[],
    )
    .expect_err("NLL must reject mutable conflict");
    assert!(
        err.contains("NLL borrow check failed"),
        "expected NLL error, got: {err}"
    );
    assert!(
        err.contains("E_BORROW_CONFLICT"),
        "expected E_BORROW_CONFLICT code, got: {err}"
    );
    assert_nll_p3_wording(&err);
    assert!(
        err.contains("修改引用") && err.contains("无法同时读取"),
        "error message should contain friendly wording, got: {err}"
    );
}

/// NLL 检测闭包穿越借用冲突：闭包按 ByRef 修改捕获容器后迭代读取。
#[test]
fn nll_strict_detects_closure_capture_mutation_conflict() {
    let err = compile_in_process(
        "ir_nll_closure_conflict",
        r#"using Arc;
using Arc.Collections;

void Main() {
    var v = new List<int>();
    v.Add(1);
    v.Add(2);
    Func<int> f = () => { v.Add(1); return v.Count; };
    foreach (var x in v) {
        var g = f();
    }
}
"#,
        &[],
    )
    .expect_err("NLL must reject closure capture borrow conflict");
    assert!(
        err.contains("NLL borrow check failed"),
        "expected NLL error, got: {err}"
    );
    assert!(
        err.contains("E_BORROW_CONFLICT"),
        "expected E_BORROW_CONFLICT code, got: {err}"
    );
    assert_nll_p3_wording(&err);
}

/// NLL 干净代码不误报（仅断言不触发 NLL 错误，与原迁移语义一致）。
#[test]
fn nll_clean_code_no_false_positive() {
    let name = "ir_nll_clean";
    if !clang_available() {
        eprintln!("skip {name}: clang not found");
        return;
    }
    let res = compile_in_process(
        name,
        r#"using Arc;
using Arc.Collections;

void Main() {
    var v = new List<int>();
    v.Add(1);
    v.Add(2);
    var sum = 0;
    foreach (var x in v) {
        sum = sum + x;
    }
    Console.WriteLine(sum);
}
"#,
        &[],
    );
    match res {
        Ok(()) => {}
        Err(e) => assert!(
            !e.contains("NLL borrow check failed"),
            "clean code must not trigger NLL false positive, got: {e}"
        ),
    }
}

/// NLL 干净闭包不误报：闭包仅读取捕获（不生成捕获借用）。
#[test]
fn nll_clean_closure_no_false_positive() {
    let name = "ir_nll_closure_clean";
    if !clang_available() {
        eprintln!("skip {name}: clang not found");
        return;
    }
    let res = compile_in_process(
        name,
        r#"using Arc;
using Arc.Collections;

void Main() {
    var v = new List<int>();
    v.Add(1);
    v.Add(2);
    Func<int> f = () => v.Count;
    var sum = f();
    foreach (var x in v) {
        sum = sum + x;
    }
    Console.WriteLine(sum);
}
"#,
        &[],
    );
    match res {
        Ok(()) => {}
        Err(e) => assert!(
            !e.contains("NLL borrow check failed"),
            "clean closure code must not trigger NLL false positive, got: {e}"
        ),
    }
}
