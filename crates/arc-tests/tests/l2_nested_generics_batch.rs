//! L2 批量：嵌套泛型/类元素容器回归集（D2 验收门，full-rt 门控）。
//!
//! stability 评审 D2（判定布局化）：`{集合}_{T}` mangle 名解析放宽后，类元素
//! 容器走「值槽 ABI」路径——#15 实证 Queue/Stack 非标量分支曾直传对象指针把
//! 对象头 refcount 快照当元素。本批以用户类元素 × 增删查覆盖 Queue/Stack/
//! Dictionary 与嵌套泛型 Queue，防同族回归。case 自打 `ARC_CASE:<name>:PASS/FAIL`。
//!
//! 备注：HashSet<T> 带 `T : IEquatable<T>` 泛型约束（typeck 实证），类元素
//! 入 Set 须实现接口——约束自身的合规面不在本批（见 D2 账本）。

#[cfg(feature = "full-rt")]
use arc_tests::assert_compiles_and_runs_batch;

#[cfg(feature = "full-rt")]
fn assert_all_passed(batch: &str, results: &[arc_tests::BatchRunResult]) {
    for r in results {
        assert!(
            r.passed,
            "{batch}: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_nested_generics_batch() {
    let results = assert_compiles_and_runs_batch(
        "nested-generics",
        &[
            (
                "ng_queue_class_elem",
                r#"using Arc;
using Arc.Collections;

class Box {
    public int V;
    public Box(int v) { this.V = v; }
}

void Main() {
    Queue<Box> q = new Queue<Box>();
    q.Enqueue(new Box(10));
    q.Enqueue(new Box(20));
    Box head = q.Peek();
    bool ok = head.V == 10;
    Box first = q.Dequeue();
    ok = ok && first.V == 10 && q.Count == 1;
    Box second = q.Dequeue();
    ok = ok && second.V == 20 && q.Count == 0;
    if (ok) {
        Console.WriteLine("ARC_CASE:ng_queue_class_elem:PASS");
    } else {
        Console.WriteLine("ARC_CASE:ng_queue_class_elem:FAIL");
    }
}
"#,
            ),
            (
                "ng_stack_class_elem",
                r#"using Arc;
using Arc.Collections;

class SBox {
    public int V;
    public SBox(int v) { this.V = v; }
}

void Main() {
    Stack<SBox> s = new Stack<SBox>();
    s.Push(new SBox(1));
    s.Push(new SBox(2));
    bool ok = s.Peek().V == 2;
    SBox top = s.Pop();
    ok = ok && top.V == 2 && s.Count == 1;
    SBox bottom = s.Pop();
    ok = ok && bottom.V == 1 && s.Count == 0;
    if (ok) {
        Console.WriteLine("ARC_CASE:ng_stack_class_elem:PASS");
    } else {
        Console.WriteLine("ARC_CASE:ng_stack_class_elem:FAIL");
    }
}
"#,
            ),
            (
                "ng_dict_class_value",
                r#"using Arc;
using Arc.Collections;

class Payload {
    public int N;
    public Payload(int n) { this.N = n; }
}

void Main() {
    Dictionary<string, Payload> d = new Dictionary<string, Payload>();
    d["k1"] = new Payload(11);
    d["k2"] = new Payload(22);
    bool ok = d["k1"].N == 11 && d["k2"].N == 22;
    ok = ok && d.Remove("k1") && d.Count == 1 && d["k2"].N == 22;
    if (ok) {
        Console.WriteLine("ARC_CASE:ng_dict_class_value:PASS");
    } else {
        Console.WriteLine("ARC_CASE:ng_dict_class_value:FAIL");
    }
}
"#,
            ),
            (
                "ng_queue_nested_int",
                r#"using Arc;
using Arc.Collections;

void Main() {
    Queue<Queue<int>> outer = new Queue<Queue<int>>();
    Queue<int> inner = new Queue<int>();
    inner.Enqueue(5);
    inner.Enqueue(6);
    outer.Enqueue(inner);
    Queue<int> got = outer.Dequeue();
    bool ok = got.Dequeue() == 5 && got.Dequeue() == 6 && got.Count == 0;
    if (ok) {
        Console.WriteLine("ARC_CASE:ng_queue_nested_int:PASS");
    } else {
        Console.WriteLine("ARC_CASE:ng_queue_nested_int:FAIL");
    }
}
"#,
            ),
        ],
    );
    assert_all_passed("nested-generics", &results);
}
