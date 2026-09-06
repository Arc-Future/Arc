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
            (
                // RFC 051 S3b：类值字典条目所有权收口回归（dict-probe13 家族）。
                // 覆盖/移除/清空/对象死亡四释放通道 + 读臂交错——双重释放
                // （0xC0000374）/提前释放（UAF）以进程崩溃/错误退出码暴露，
                // 校验和防静默假绿。2000 轮 × 32 值 ≈ 字典全生命周期 64k 条目。
                "ng_dict_class_value_retention",
                r#"using Arc;
using Arc.Collections;

class RetentionPayload {
    public int N;
    public RetentionPayload(int n) { this.N = n; }
}

void Main() {
    long sink = 0;
    for (int round = 0; round < 2000; round++) {
        Dictionary<int, RetentionPayload> d = new Dictionary<int, RetentionPayload>(64);
        for (int i = 0; i < 32; i++) {
            d.Add(i, new RetentionPayload(round * 100 + i));
        }
        for (int i = 0; i < 32; i = i + 2) {
            sink = sink + d[i].N;
            if (!d.ContainsKey(i)) {
                Console.WriteLine("ARC_CASE:ng_dict_class_value_retention:FAIL");
                return;
            }
        }
        for (int i = 0; i < 32; i = i + 4) {
            d[i] = new RetentionPayload(round * 200 + i);
        }
        for (int i = 0; i < 32; i = i + 4) {
            if (!d.Remove(i)) {
                Console.WriteLine("ARC_CASE:ng_dict_class_value_retention:FAIL");
                return;
            }
        }
        d.Clear();
        if (d.Count != 0) {
            Console.WriteLine("ARC_CASE:ng_dict_class_value_retention:FAIL");
            return;
        }
        for (int i = 0; i < 8; i++) {
            d[i] = new RetentionPayload(round * 300 + i);
        }
        sink = sink + d.Count;
    }
    if (sink == 0) {
        Console.WriteLine("ARC_CASE:ng_dict_class_value_retention:FAIL");
    } else {
        Console.WriteLine("ARC_CASE:ng_dict_class_value_retention:PASS");
    }
}
"#,
            ),
            (
                // RFC 051 S3c：SortedDictionary 类值所有权回归（rt_sorted_dict
                // create_owned + 包装类 finalizer）。Fill 助手中 new 的 Payload
                // 在助手出口失去调用方引用；修复前 sorted 从不 retain → 值悬垂，
                // 堆复用后回读为垃圾（sort-probe1f 实测 sum=57625126 vs 2016）。
                // 本 case：回读校验和 + 覆盖/移除/清空/死亡四通道 + 复用潮，
                // 悬垂/双释放以崩溃或校验失配暴露。
                "ng_sorted_dict_class_value",
                r#"using Arc;
using Arc.Collections;

class SortedPayload {
    public int N;
    public SortedPayload(int n) { this.N = n; }
}

void FillSorted(SortedDictionary<int, SortedPayload> d, int seedBase) {
    for (int i = 0; i < 32; i++) {
        d[i] = new SortedPayload(seedBase + i);
    }
}

void ChurnPayloads(int rounds) {
    // 同尺寸对象分配/释放潮——迫使堆复用（悬垂值在此暴露为垃圾回读）
    for (int r = 0; r < rounds; r++) {
        SortedPayload p = new SortedPayload(900000 + r);
        p.N = p.N + 1;
    }
}

void Main() {
    for (int round = 0; round < 300; round++) {
        SortedDictionary<int, SortedPayload> d = new SortedDictionary<int, SortedPayload>();
        int seedVal = round * 100;
        FillSorted(d, seedVal);
        ChurnPayloads(100);
        long sum = 0;
        for (int i = 0; i < 32; i++) {
            SortedPayload p = d[i];
            if (p == null) {
                Console.WriteLine("ARC_CASE:ng_sorted_dict_class_value:FAIL:null@" + round);
                return;
            }
            sum = sum + p.N;
        }
        long expected = 0;
        for (int k = 0; k < 32; k++) {
            expected = expected + (seedVal + k);
        }
        if (sum != expected) {
            Console.WriteLine("ARC_CASE:ng_sorted_dict_class_value:FAIL:sum=" + sum);
            return;
        }
        for (int i = 0; i < 32; i = i + 4) {
            d[i] = new SortedPayload(round * 300 + i);
        }
        for (int i = 0; i < 32; i = i + 4) {
            if (!d.Remove(i)) {
                Console.WriteLine("ARC_CASE:ng_sorted_dict_class_value:FAIL:remove");
                return;
            }
        }
        d.Clear();
        if (d.Count != 0) {
            Console.WriteLine("ARC_CASE:ng_sorted_dict_class_value:FAIL:clear");
            return;
        }
    }
    Console.WriteLine("ARC_CASE:ng_sorted_dict_class_value:PASS");
}
"#,
            ),
            (
                // RFC 051 S3d：ConcurrentDictionary 类值所有权收口（owned
                // create 变体 + 存储侧锁内 +1 + 读臂锁内借用 + TryRemove 移交）。
                // 本 case 单线程覆盖协议全通道：TryAdd 插入存储 +1、TryGetValue/
                // GetOrAdd 命中借用（runtime 锁内 retain）、indexer 覆盖旧值释放、
                // TryUpdate CAS 替换、TryRemove out 移交（二次移除 false + out
                // null）、Clear、包装类死亡（每轮新字典）——配同尺寸对象复用潮，
                // 借还失配/悬垂以崩溃或校验和漂移暴露。
                "ng_concurrent_dict_class_value",
                r#"using Arc;
using Arc.Collections;

class ConcurrentPayload {
    public int N;
    public ConcurrentPayload(int n) { this.N = n; }
}

void ChurnConcurrentPayloads(int rounds) {
    // 同尺寸对象分配/释放潮——迫使堆复用（悬垂值在此暴露为垃圾回读）
    for (int r = 0; r < rounds; r++) {
        ConcurrentPayload p = new ConcurrentPayload(900000 + r);
        p.N = p.N + 1;
    }
}

void Main() {
    for (int round = 0; round < 200; round++) {
        ConcurrentDictionary<int, ConcurrentPayload?> d =
            new ConcurrentDictionary<int, ConcurrentPayload?>();
        int seedVal = round * 100;
        for (int i = 0; i < 32; i++) {
            if (!d.TryAdd(i, new ConcurrentPayload(seedVal + i))) {
                Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:add@" + round);
                return;
            }
        }
        if (d.TryAdd(0, new ConcurrentPayload(0))) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:dup-add");
            return;
        }
        ChurnConcurrentPayloads(50);
        long sum = 0;
        for (int i = 0; i < 32; i++) {
            ConcurrentPayload? p = null;
            if (!d.TryGetValue(i, out p) || p == null) {
                Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:get@" + round);
                return;
            }
            sum = sum + p.N;
        }
        long expected = 0;
        for (int k = 0; k < 32; k++) {
            expected = expected + (seedVal + k);
        }
        if (sum != expected) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:sum=" + sum);
            return;
        }
        ConcurrentPayload? hit = d.GetOrAdd(0, new ConcurrentPayload(0));
        if (hit == null || hit.N != seedVal) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:goadd-hit");
            return;
        }
        ConcurrentPayload? fresh = d.GetOrAdd(100, new ConcurrentPayload(seedVal + 100));
        if (fresh == null || fresh.N != seedVal + 100) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:goadd-miss");
            return;
        }
        d[0] = new ConcurrentPayload(round * 300);
        ConcurrentPayload? over = d[0];
        if (over == null || over.N != round * 300) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:overwrite");
            return;
        }
        ConcurrentPayload? old1 = d.GetValueOrDefault(1);
        if (!d.TryUpdate(1, new ConcurrentPayload(round * 500), old1)) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:update");
            return;
        }
        ConcurrentPayload? cur1 = d.GetValueOrDefault(1);
        if (cur1 == null || cur1.N != round * 500) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:update-cur");
            return;
        }
        if (d.TryUpdate(1, new ConcurrentPayload(round * 700), old1)) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:stale-cas");
            return;
        }
        ConcurrentPayload? rem = null;
        if (!d.TryRemove(2, out rem) || rem == null || rem.N != seedVal + 2) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:remove");
            return;
        }
        ConcurrentPayload? rem2 = null;
        if (d.TryRemove(2, out rem2) || rem2 != null) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:re-remove");
            return;
        }
        d.Clear();
        if (d.Count != 0) {
            Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:FAIL:clear");
            return;
        }
    }
    Console.WriteLine("ARC_CASE:ng_concurrent_dict_class_value:PASS");
}
"#,
            ),
        ],
    );
    assert_all_passed("nested-generics", &results);
}
