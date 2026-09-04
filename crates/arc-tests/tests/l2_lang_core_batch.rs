//! L2 批量运行时测试：验证语言核心特性的真实运行时行为。
//!
//! 通过 `build_and_run_batch` 合并多个 case 为一次编译 + 一次运行。
//! 每个 case 自行输出 `ARC_CASE:{name}:PASS/FAIL` 标记。
//! 需 `--features full-rt` 门控，默认 `cargo test` 不触发。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch, BatchCase};

#[test]
fn lang_core_batch() {
    let results = build_and_run_batch(
        "lang_core",
        &[
            BatchCase {
                name: "bitwise_ops",
                src: r#"using Arc;

void Main() {
    int a = 0b1100;
    int b = 0b1010;
    if ((a & b) != 0b1000) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:and"); return; }
    if ((a | b) != 0b1110) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:or"); return; }
    if ((a ^ b) != 0b0110) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:xor"); return; }
    Console.WriteLine("ARC_CASE:bitwise_ops:PASS");
}
"#,
            },
            BatchCase {
                name: "compound_assign",
                src: r#"using Arc;

void Main() {
    int a = 10;
    a += 3;
    if (a != 13) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:add"); return; }
    a -= 5;
    if (a != 8) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:sub"); return; }
    a *= 2;
    if (a != 16) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:mul"); return; }
    Console.WriteLine("ARC_CASE:compound_assign:PASS");
}
"#,
            },
            BatchCase {
                name: "switch_expr",
                src: r#"using Arc;

void Main() {
    int n = 2;
    string s = n switch { 1 => "one", 2 => "two", _ => "other" };
    if (!(s == "two")) { Console.WriteLine("ARC_CASE:switch_expr:FAIL:switch"); return; }
    Console.WriteLine("ARC_CASE:switch_expr:PASS");
}
"#,
            },
            BatchCase {
                name: "math_round_bankers",
                src: r#"using Arc;

void Main() {
    // C# Math.Round 默认银行家舍入（round-half-to-even）：中点向偶数。
    // 2.5→2 / 0.5→0 / -2.5→-2 可区分 llvm.rint（to-even）与 llvm.round（away-from-zero）。
    if (Math.Round(2.5) != 2.0) { Console.WriteLine("ARC_CASE:math_round_bankers:FAIL:2.5"); return; }
    if (Math.Round(3.5) != 4.0) { Console.WriteLine("ARC_CASE:math_round_bankers:FAIL:3.5"); return; }
    if (Math.Round(0.5) != 0.0) { Console.WriteLine("ARC_CASE:math_round_bankers:FAIL:0.5"); return; }
    if (Math.Round(-2.5) != -2.0) { Console.WriteLine("ARC_CASE:math_round_bankers:FAIL:-2.5"); return; }
    if (Math.Round(2.4) != 2.0) { Console.WriteLine("ARC_CASE:math_round_bankers:FAIL:2.4"); return; }
    if (Math.Round(2.6) != 3.0) { Console.WriteLine("ARC_CASE:math_round_bankers:FAIL:2.6"); return; }
    Console.WriteLine("ARC_CASE:math_round_bankers:PASS");
}
"#,
            },
            BatchCase {
                name: "arg_exception_message",
                src: r#"using Arc;

void Main() {
    // 一参 ctor：Message 必须是 spec 文案 + 参数名后缀，不得误用 paramName 本身。
    try {
        throw new ArgumentNullException("buf");
    } catch (ArgumentNullException e) {
        if (e.Message != "Value cannot be null. (Parameter 'buf')") { Console.WriteLine("ARC_CASE:arg_exception_message:FAIL:anre_msg"); return; }
        if (e.ParamName != "buf") { Console.WriteLine("ARC_CASE:arg_exception_message:FAIL:anre_param"); return; }
    }
    try {
        throw new ArgumentOutOfRangeException("idx");
    } catch (ArgumentOutOfRangeException e) {
        if (e.Message != "Specified argument was out of range of valid values. (Parameter 'idx')") { Console.WriteLine("ARC_CASE:arg_exception_message:FAIL:aore_msg"); return; }
        if (e.ParamName != "idx") { Console.WriteLine("ARC_CASE:arg_exception_message:FAIL:aore_param"); return; }
    }
    Console.WriteLine("ARC_CASE:arg_exception_message:PASS");
}
"#,
            },
            BatchCase {
                name: "runtime_type_members",
                src: r#"using Arc;
using Arc.Reflection;

class Widget {
    public int Size;
    public string Tag;
    public int Score { get; set; }
    public int GetScore() { return 1; }
    public void Reset() { }
}

void Main() {
    // GetMembers 真实现：Count == GetMethods + GetFields + GetProperties 之和
    // （自洽判别，stub 时代恒为 0）；expected >= 5 防三源同空的自洽假绿。
    var t = typeof(Widget);
    int expected = t.GetMethods().Count + t.GetFields().Count + t.GetProperties().Count;
    if (expected < 5) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:sources:" + expected); return; }
    var members = t.GetMembers();
    if (members.Count != expected) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:count:" + members.Count + "!=" + expected); return; }

    // 未拦截的假数据成员已诚实化：一律抛 NotImplementedException（不再空 List/false）
    bool threw = false;
    try { t.GetEvents(); } catch (NotImplementedException e) { threw = true; }
    if (!threw) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:get_events"); return; }
    threw = false;
    try { t.GetConstructors(); } catch (NotImplementedException e) { threw = true; }
    if (!threw) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:get_ctors"); return; }
    threw = false;
    try { t.GetCustomAttributes(); } catch (NotImplementedException e) { threw = true; }
    if (!threw) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:get_attrs"); return; }
    threw = false;
    try { t.IsDefined(t); } catch (NotImplementedException e) { threw = true; }
    if (!threw) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:is_defined"); return; }

    List<EventInfo> ev = null;
    threw = false;
    try { ev = t.DeclaredEvents; } catch (NotImplementedException e) { threw = true; }
    if (!threw || ev != null) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:declared_events"); return; }
    List<ConstructorInfo> ctors = null;
    threw = false;
    try { ctors = t.DeclaredConstructors; } catch (NotImplementedException e) { threw = true; }
    if (!threw || ctors != null) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:declared_ctors"); return; }
    List<MemberInfo> dm = null;
    threw = false;
    try { dm = t.DeclaredMembers; } catch (NotImplementedException e) { threw = true; }
    if (!threw || dm != null) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:declared_members"); return; }
    List<Type> nested = null;
    threw = false;
    try { nested = t.DeclaredNestedTypes; } catch (NotImplementedException e) { threw = true; }
    if (!threw || nested != null) { Console.WriteLine("ARC_CASE:runtime_type_members:FAIL:declared_nested"); return; }

    Console.WriteLine("ARC_CASE:runtime_type_members:PASS");
}
"#,
            },
            BatchCase {
                name: "primitive_typeof_and_boxing",
                src: r#"using Arc;
using Arc.Reflection;

void Main() {
    // RFC 017 阶段一：基元 typeinfo 经 rt_typeinfo_prim 函数符号静态查询，
    // 数值基元 kind = Primitive（name 为语言关键字）；string/object 为 CLASS。
    Type ti = typeof(int);
    if (ti == null) { Console.WriteLine("ARC_CASE:primitive_typeof_and_boxing:FAIL:null_int"); return; }
    if (ti.Name != "int") { Console.WriteLine("ARC_CASE:primitive_typeof_and_boxing:FAIL:int_name:" + ti.Name); return; }
    if (!ti.IsPrimitive) { Console.WriteLine("ARC_CASE:primitive_typeof_and_boxing:FAIL:int_kind"); return; }

    Type ts = typeof(string);
    if (ts == null || ts.Name != "string" || ts.IsPrimitive) { Console.WriteLine("ARC_CASE:primitive_typeof_and_boxing:FAIL:string"); return; }

    // 基元装箱 vtable 经 rt_box_vtable 函数符号查询（数据符号零跨边界）。
    object boxed = 42;
    int unboxed = (int)boxed;
    if (unboxed != 42) { Console.WriteLine("ARC_CASE:primitive_typeof_and_boxing:FAIL:unbox:" + unboxed); return; }

    object boxedStr = "arc";
    if ((string)boxedStr != "arc") { Console.WriteLine("ARC_CASE:primitive_typeof_and_boxing:FAIL:unbox_str"); return; }

    Console.WriteLine("ARC_CASE:primitive_typeof_and_boxing:PASS");
}
"#,
            },
        ],
    );

    let r = batch_case_result(&results, "bitwise_ops");
    assert!(
        r.passed,
        "bitwise_ops failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "compound_assign");
    assert!(
        r.passed,
        "compound_assign failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "switch_expr");
    assert!(
        r.passed,
        "switch_expr failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "math_round_bankers");
    assert!(
        r.passed,
        "math_round_bankers failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "arg_exception_message");
    assert!(
        r.passed,
        "arg_exception_message failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "runtime_type_members");
    assert!(
        r.passed,
        "runtime_type_members failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "primitive_typeof_and_boxing");
    assert!(
        r.passed,
        "primitive_typeof_and_boxing failed: {:?} stdout: {}",
        r.error, r.stdout
    );
}

#[test]
fn struct_copy_and_coalesce_assign_batch() {
    // RFC 005 自动 Copy + `??=`：C# 值语义端到端——struct 赋值为逐字段
    // 复制（改目标不影响源），`??=` 仅在左值为 null 时写入。
    let results = build_and_run_batch(
        "struct_copy_coalesce",
        &[
            BatchCase {
                name: "struct_copy_semantics",
                src: r#"using Arc;

struct Point { public int X; public int Y; }

void Main() {
    var a = new Point() { X = 1, Y = 2 };
    var b = a;
    b.X = 99;
    if (a.X != 1) { Console.WriteLine("ARC_CASE:struct_copy_semantics:FAIL:alias"); return; }
    if (b.X != 99) { Console.WriteLine("ARC_CASE:struct_copy_semantics:FAIL:copy"); return; }
    Console.WriteLine("ARC_CASE:struct_copy_semantics:PASS");
}
"#,
            },
            BatchCase {
                name: "null_coalesce_assign",
                src: r#"using Arc;

void Main() {
    string? a = null;
    a ??= "fallback";
    if (a == null) { Console.WriteLine("ARC_CASE:null_coalesce_assign:FAIL:null_branch"); return; }
    if (a != "fallback") { Console.WriteLine("ARC_CASE:null_coalesce_assign:FAIL:ne_cmp"); return; }
    a ??= "other";
    if (a == "other") { Console.WriteLine("ARC_CASE:null_coalesce_assign:FAIL:nonnull_branch"); return; }
    if (a != "fallback") { Console.WriteLine("ARC_CASE:null_coalesce_assign:FAIL:retain"); return; }
    string b = "fall";
    b += "back";
    string? c = null;
    c ??= b;
    if (c != "fallback") { Console.WriteLine("ARC_CASE:null_coalesce_assign:FAIL:heap_cmp"); return; }
    Console.WriteLine("ARC_CASE:null_coalesce_assign:PASS");
}
"#,
            },
        ],
    );

    let r = batch_case_result(&results, "struct_copy_semantics");
    assert!(
        r.passed,
        "struct_copy_semantics failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "null_coalesce_assign");
    assert!(
        r.passed,
        "null_coalesce_assign failed: {:?} stdout: {}",
        r.error, r.stdout
    );
}
