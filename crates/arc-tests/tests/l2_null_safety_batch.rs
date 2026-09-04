//! L2 批量运行时测试：`?.` / `!.` 链式访问（RFC 009 L2）端到端语义。
//!
//! 重点回归 MIR lower 的嵌套 receiver 物化通道：`a?.Next?.Tag` 的外层
//! receiver（`a?.Next`）需经 with_binary 层物化为临时 local + 伪 Ident 重写，
//! 否则 operand_from_expr 对嵌套 NullCond/ForceDeref panic。
//! 每个 case 自行输出 `ARC_CASE:{name}:PASS/FAIL` 标记。
//! 需 `--features full-rt` 门控，默认 `cargo test` 不触发。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch, BatchCase};

#[test]
fn null_safety_chain_batch() {
    let results = build_and_run_batch(
        "null_safety_chain",
        &[
            BatchCase {
                name: "null_cond_chain_field",
                src: r#"using Arc;

class ChainNode {
    public string Tag = "leaf";
    public ChainNode? Next;
}

void Main() {
    ChainNode? a = new ChainNode();
    a.Tag = "a";
    ChainNode? b = new ChainNode();
    b.Tag = "b";
    a.Next = b;

    // 非空路径：a.Next = b → b.Tag
    string v1 = a?.Next?.Tag ?? "none";
    if (v1 != "b") { Console.WriteLine("ARC_CASE:null_cond_chain_field:FAIL:nonnull"); return; }

    // 中间 null：c.Next 为 null → 第二段短路
    ChainNode? c = new ChainNode();
    c.Tag = "c";
    string v2 = c?.Next?.Tag ?? "midnull";
    if (v2 != "midnull") { Console.WriteLine("ARC_CASE:null_cond_chain_field:FAIL:midnull"); return; }

    // 头 null：第一段短路
    ChainNode? n = null;
    string v3 = n?.Next?.Tag ?? "headnull";
    if (v3 != "headnull") { Console.WriteLine("ARC_CASE:null_cond_chain_field:FAIL:headnull"); return; }

    // 三层链：外层 receiver `a?.Next?.Next` 仍为 NullCond（双重物化），b.Next 为 null
    string v4 = a?.Next?.Next?.Tag ?? "deepnull";
    if (v4 != "deepnull") { Console.WriteLine("ARC_CASE:null_cond_chain_field:FAIL:deepnull"); return; }

    Console.WriteLine("ARC_CASE:null_cond_chain_field:PASS");
}
"#,
            },
            BatchCase {
                name: "null_cond_method_chain",
                src: r#"using Arc;

class MethodNode {
    public string Tag = "leaf";
    public MethodNode? Next;
    public MethodNode? GetNext() { return Next; }
}

void Main() {
    MethodNode? a = new MethodNode();
    a.Tag = "a";
    MethodNode? b = new MethodNode();
    b.Tag = "b";
    a.Next = b;

    // 非空方法链：a?.GetNext()?.Tag → NullCondMethod 与 NullCondField 嵌套
    string v1 = a?.GetNext()?.Tag ?? "none";
    if (v1 != "b") { Console.WriteLine("ARC_CASE:null_cond_method_chain:FAIL:nonnull"); return; }

    // 方法返回 null：中间短路
    MethodNode? c = new MethodNode();
    c.Tag = "c";
    string v2 = c?.GetNext()?.Tag ?? "midnull";
    if (v2 != "midnull") { Console.WriteLine("ARC_CASE:null_cond_method_chain:FAIL:midnull"); return; }

    // 头 null：不调用 GetNext
    MethodNode? n = null;
    string v3 = n?.GetNext()?.Tag ?? "headnull";
    if (v3 != "headnull") { Console.WriteLine("ARC_CASE:null_cond_method_chain:FAIL:headnull"); return; }

    Console.WriteLine("ARC_CASE:null_cond_method_chain:PASS");
}
"#,
            },
            BatchCase {
                name: "force_deref_chain",
                src: r#"using Arc;

class DerefNode {
    public string Tag = "leaf";
    public DerefNode? Next;
    public DerefNode? GetNext() { return Next; }
}

void Main() {
    DerefNode? a = new DerefNode();
    a.Tag = "a";
    DerefNode? b = new DerefNode();
    b.Tag = "b";
    a.Next = b;

    // !. 字段链：receiver Ident 为简单形式，直通路径
    string t1 = a!.Next.Tag;
    if (t1 != "b") { Console.WriteLine("ARC_CASE:force_deref_chain:FAIL:field"); return; }

    // `?.` 与 `!.` 混合链：外层 receiver `a?.Next` 为 NullCond，需物化
    string t2 = a?.Next!.Tag;
    if (t2 != "b") { Console.WriteLine("ARC_CASE:force_deref_chain:FAIL:mixed"); return; }

    // !. 方法调用：ForceDerefMethod 直通路径
    string t3 = a!.GetNext().Tag;
    if (t3 != "b") { Console.WriteLine("ARC_CASE:force_deref_chain:FAIL:method"); return; }

    Console.WriteLine("ARC_CASE:force_deref_chain:PASS");
}
"#,
            },
        ],
    );

    let r = batch_case_result(&results, "null_cond_chain_field");
    assert!(
        r.passed,
        "null_cond_chain_field failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "null_cond_method_chain");
    assert!(
        r.passed,
        "null_cond_method_chain failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "force_deref_chain");
    assert!(
        r.passed,
        "force_deref_chain failed: {:?} stdout: {}",
        r.error, r.stdout
    );
}
