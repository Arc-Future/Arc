//! L1 批量：async 状态机回归集（5 case，#[ignore] 因 GAP #6）。
//!
//! 这些用例验证 async 状态机正确性，但在非 Windows 目标上
//! codegen 失败（cleanuppad 需要 EH personality）。
//! 修复 GAP #6 后移除 #[ignore]。

use arc_tests::assert_compiles_batch;

#[test]
#[ignore = "GAP #6: async state machine codegen needs EH personality on non-Windows"]
fn compiles_async_state_machine_batch() {
    assert_compiles_batch(
        "async_state_machine",
        &[
            (
                "multi_await",
                r#"using Arc;

async Task<string> GetName() {
    return "Alice";
}

async Task<int> GetAge() {
    return 18;
}

async Task<int> GetScore() {
    return 95;
}

async Task<void> Main() {
    var name = await GetName();
    var age = await GetAge();
    var score = await GetScore();
    Console.WriteLine(name);
    if (age == 18) {
        Console.WriteLine("age ok");
    }
    if (score == 95) {
        Console.WriteLine("score ok");
    }
    if (age == 18 && score == 95) {
        Console.WriteLine("sm multi-await ok");
    }
}
"#,
            ),
            (
                "single_await",
                r#"using Arc;

async Task<int> FetchValue() {
    return 42;
}

async Task<void> Main() {
    var value = await FetchValue();
    if (value == 42) {
        Console.WriteLine("sm single ok");
    }
}
"#,
            ),
            (
                "cross_await_local_survival",
                r#"using Arc;

async Task<int> GetFirst() {
    return 100;
}

async Task<int> GetSecond() {
    return 200;
}

async Task<void> Main() {
    var first = await GetFirst();
    var second = await GetSecond();
    var sum = first + second;
    if (sum == 300) {
        Console.WriteLine("cross await ok");
    }
}
"#,
            ),
            (
                "mixed_types",
                r#"using Arc;

async Task<string> FetchLabel() {
    return "count";
}

async Task<int> FetchCount() {
    return 7;
}

async Task<void> Main() {
    var label = await FetchLabel();
    var count = await FetchCount();
    Console.WriteLine(label);
    if (count == 7) {
        Console.WriteLine("count ok");
    }
}
"#,
            ),
            (
                "arc_pairing_rfc103",
                r#"using Arc;

class Node {
    public string Name;
    public Node(string n) { Name = n; }
}

async Task<Node> MakeNode(string n) { return new Node(n); }

async Task<string> Describe(Node n) {
    var tag = n.Name;
    var a = await MakeNode("Alpha");
    var b = await MakeNode("Beta");
    return tag;
}

async Task<void> Main() {
    var n1 = await MakeNode("Alpha");
    var n2 = await MakeNode("Beta");
    Console.WriteLine(n1.Name);
    Console.WriteLine(n2.Name);
    var d = await Describe(n1);
    Console.WriteLine(d);
}
"#,
            ),
        ],
    );
}
