//! L2 批量：并发/线程运行时回归集（29 case，6 批隔离）。
//!
//! 从 arc-integration 迁移（a2627a0f 退场承接）：interlocked、lock_statement、
//! concurrent_eh、concurrent_deepen、concurrent_facade_snapshot、
//! blocking_collection_pcc_ctor、threadpool_scheduler（5 sync + 3 async）、
//! cycle_collection_thread。case 按批量协议自打
//! `ARC_CASE:<name>:PASS/FAIL:<msg>` 标记，消费返回值逐 case 断言。
//! async case（`async Task<void> Main`）由批量 driver 以 `await` 调度。
//! 通过 `--features full-rt` 门控。
//!
//! 未迁移：10 个 rt_* C API 白盒与 parallel_build（CLI workspace 构建
//! 确定性）不入本框架。

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
fn runs_concurrency_core_batch() {
    let results = assert_compiles_and_runs_batch(
        "concurrency_core",
        &[
            (
                "interlocked_int_surface",
                r#"using Arc;
using Arc.Threading;

void Main() {
    int x = 10;
    int n = Interlocked.Increment(ref x);
    int old = Interlocked.Exchange(ref x, 100);
    int hit = Interlocked.CompareExchange(ref x, 200, 100);
    int miss = Interlocked.CompareExchange(ref x, 1, 100);
    if (n == 11 && old == 11 && hit == 100 && x == 200 && miss == 200) {
        Console.WriteLine("ARC_CASE:interlocked_int_surface:PASS");
    } else {
        Console.WriteLine("ARC_CASE:interlocked_int_surface:FAIL:values");
    }
}
"#,
            ),
            (
                "thread_sleep_positive_ms",
                r#"using Arc;
using Arc.Diagnostics;
using Arc.Threading;

void Main() {
    Stopwatch sw = Stopwatch.StartNew();
    Thread.Sleep(30);
    sw.Stop();
    if (sw.ElapsedMilliseconds >= 15) {
        Console.WriteLine("ARC_CASE:thread_sleep_positive_ms:PASS");
    } else {
        Console.WriteLine("ARC_CASE:thread_sleep_positive_ms:FAIL:elapsed=" + sw.ElapsedMilliseconds);
    }
}
"#,
            ),
            (
                "lock_statement",
                r#"using Arc;
using Arc.Threading;

void Main() {
    Lock l = new Lock();
    int x = 0;
    lock (l) {
        x = 42;
    }
    if (x == 42) {
        Console.WriteLine("ARC_CASE:lock_statement:PASS");
    } else {
        Console.WriteLine("ARC_CASE:lock_statement:FAIL:x=" + x);
    }
}
"#,
            ),
            (
                "concurrent_eh",
                r#"using Arc;
using Arc.Collections.Concurrent;
using Arc.Threading;

void Worker(string tag, int count, ConcurrentQueue<string> errs) {
    int finallyRuns = 0;
    for (int i = 0; i < count; i++) {
        try {
            try {
                throw new ArgumentException(tag + "-" + i);
            } finally {
                finallyRuns++;
            }
        } catch (ArgumentException e) {
            if (e.Message != tag + "-" + i) {
                errs.Enqueue(tag + ":msg-mismatch:" + i);
                return;
            }
        }
    }
    if (finallyRuns != count) {
        errs.Enqueue(tag + ":finally-runs:" + finallyRuns);
    }
}

void Main() {
    ConcurrentQueue<string> errs = new ConcurrentQueue<string>();
    int n = 200;
    Thread t0 = new Thread(() => Worker("t0", n, errs));
    Thread t1 = new Thread(() => Worker("t1", n, errs));
    Thread t2 = new Thread(() => Worker("t2", n, errs));
    Thread t3 = new Thread(() => Worker("t3", n, errs));
    t0.Start();
    t1.Start();
    t2.Start();
    t3.Start();
    t0.Join();
    t1.Join();
    t2.Join();
    t3.Join();
    if (errs.Count == 0) {
        Console.WriteLine("ARC_CASE:concurrent_eh:PASS");
    } else {
        string first = "";
        errs.TryPeek(out first);
        Console.WriteLine("ARC_CASE:concurrent_eh:FAIL:count=" + errs.Count + " first=" + first);
    }
}
"#,
            ),
            (
                // std P3: Release(int) 批量归还（§7.3 登记，Yamux 字节级流控前置）。
                // 单线程确定性设计：Wait(0) 即 TryWait 非阻塞探测。
                "semaphore_release_n",
                r#"using Arc;
using Arc.Threading;

void Main() {
    Semaphore sem = new Semaphore(0, 16);
    sem.Release(3);
    if (!sem.Wait(0)) { Console.WriteLine("ARC_CASE:semaphore_release_n:FAIL:wait1"); return; }
    if (!sem.Wait(0)) { Console.WriteLine("ARC_CASE:semaphore_release_n:FAIL:wait2"); return; }
    if (!sem.Wait(0)) { Console.WriteLine("ARC_CASE:semaphore_release_n:FAIL:wait3"); return; }
    if (sem.Wait(0)) { Console.WriteLine("ARC_CASE:semaphore_release_n:FAIL:overshoot"); return; }
    sem.Release(2);
    if (!sem.Wait(0)) { Console.WriteLine("ARC_CASE:semaphore_release_n:FAIL:wait4"); return; }
    if (!sem.Wait(0)) { Console.WriteLine("ARC_CASE:semaphore_release_n:FAIL:wait5"); return; }
    if (sem.Wait(0)) { Console.WriteLine("ARC_CASE:semaphore_release_n:FAIL:overshoot2"); return; }
    sem.Release();
    if (!sem.Wait(0)) { Console.WriteLine("ARC_CASE:semaphore_release_n:FAIL:wait6"); return; }
    sem.Dispose();
    Console.WriteLine("ARC_CASE:semaphore_release_n:PASS");
}
"#,
            ),
        ],
    );
    assert_all_passed("concurrency_core", &results);
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_concurrency_collections_batch() {
    let results = assert_compiles_and_runs_batch(
        "concurrency_collections",
        &[
            (
                "dict_compound",
                r#"using Arc;
using Arc.Collections.Concurrent;

void Main() {
    ConcurrentDictionary<string, int> d = new ConcurrentDictionary<string, int>();
    int added = d.GetOrAdd("a", 10);
    int again = d.GetOrAdd("a", 99);
    bool ok = d.TryUpdate("a", 20, 10);
    bool fail = d.TryUpdate("a", 1, 10);
    int removed;
    bool rm = d.TryRemove("a", out removed);
    d.TryAdd("k", 1);
    string[] keys = d.Keys;
    if (added == 10 && again == 10 && ok && !fail && rm && removed == 20 && keys.Length == 1) {
        Console.WriteLine("ARC_CASE:dict_compound:PASS");
    } else {
        Console.WriteLine("ARC_CASE:dict_compound:FAIL:values");
    }
}
"#,
            ),
            (
                "stack_peek_lifo",
                r#"using Arc;
using Arc.Collections.Concurrent;

void Main() {
    ConcurrentStack<int> s = new ConcurrentStack<int>();
    s.Push(10);
    s.Push(20);
    s.Push(30);
    int peek;
    bool okPeek = s.TryPeek(out peek);
    int a;
    bool okPop = s.TryPop(out a);
    if (okPeek && peek == 30 && okPop && a == 30 && s.Count == 2) {
        Console.WriteLine("ARC_CASE:stack_peek_lifo:PASS");
    } else {
        Console.WriteLine("ARC_CASE:stack_peek_lifo:FAIL:values");
    }
}
"#,
            ),
            (
                "queue_peek_to_array",
                r#"using Arc;
using Arc.Collections.Concurrent;

void Main() {
    ConcurrentQueue<int> q = new ConcurrentQueue<int>();
    q.Enqueue(1);
    q.Enqueue(2);
    int peek;
    bool ok = q.TryPeek(out peek);
    int[] arr = q.ToArray();
    if (ok && peek == 1 && arr.Length == 2 && q.Count == 2) {
        Console.WriteLine("ARC_CASE:queue_peek_to_array:PASS");
    } else {
        Console.WriteLine("ARC_CASE:queue_peek_to_array:FAIL:values");
    }
}
"#,
            ),
            (
                "try_lock_try_enter",
                r#"using Arc;
using Arc.Threading;

void Main() {
    Mutex m = new Mutex();
    bool a = m.TryLock();
    bool b = m.TryLock();
    m.Unlock();
    bool c = m.TryLock();
    m.Unlock();

    Lock l = new Lock();
    bool d = Monitor.TryEnter(l);
    Monitor.Exit(l);
    bool f = Monitor.TryEnter(l);
    Monitor.Exit(l);

    if (a && !b && c && d && f) {
        Console.WriteLine("ARC_CASE:try_lock_try_enter:PASS");
    } else {
        Console.WriteLine("ARC_CASE:try_lock_try_enter:FAIL:values");
    }
}
"#,
            ),
            (
                "queue_to_array_fifo",
                r#"using Arc.Collections.Concurrent;

void Main() {
    ConcurrentQueue<string> q = new ConcurrentQueue<string>();
    q.Enqueue("a");
    q.Enqueue("b");
    q.Enqueue("c");
    string[] arr = q.ToArray();
    if (arr.Length != 3) { Console.WriteLine("ARC_CASE:queue_to_array_fifo:FAIL:len=" + arr.Length); return; }
    if (arr[0] != "a" || arr[1] != "b" || arr[2] != "c") { Console.WriteLine("ARC_CASE:queue_to_array_fifo:FAIL:contents"); return; }
    Console.WriteLine("ARC_CASE:queue_to_array_fifo:PASS");
}
"#,
            ),
            (
                "stack_to_array_lifo",
                r#"using Arc.Collections.Concurrent;

void Main() {
    ConcurrentStack<string> st = new ConcurrentStack<string>();
    st.Push("a");
    st.Push("b");
    st.Push("c");
    string[] arr = st.ToArray();
    if (arr.Length != 3) { Console.WriteLine("ARC_CASE:stack_to_array_lifo:FAIL:len=" + arr.Length); return; }
    if (arr[0] != "c" || arr[1] != "b" || arr[2] != "a") { Console.WriteLine("ARC_CASE:stack_to_array_lifo:FAIL:contents"); return; }
    Console.WriteLine("ARC_CASE:stack_to_array_lifo:PASS");
}
"#,
            ),
            (
                "bag_to_array_contains_all",
                r#"using Arc.Collections.Concurrent;

void Main() {
    ConcurrentBag<string> bag = new ConcurrentBag<string>();
    bag.Add("x");
    bag.Add("y");
    bag.Add("z");
    string[] arr = bag.ToArray();
    if (arr.Length != 3) { Console.WriteLine("ARC_CASE:bag_to_array_contains_all:FAIL:len=" + arr.Length); return; }
    bool hasX = false; bool hasY = false; bool hasZ = false;
    for (int i = 0; i < arr.Length; i++) {
        if (arr[i] == "x") { hasX = true; }
        else if (arr[i] == "y") { hasY = true; }
        else if (arr[i] == "z") { hasZ = true; }
    }
    if (!hasX || !hasY || !hasZ) { Console.WriteLine("ARC_CASE:bag_to_array_contains_all:FAIL:contents"); return; }
    Console.WriteLine("ARC_CASE:bag_to_array_contains_all:PASS");
}
"#,
            ),
            (
                "dict_keys_snapshot",
                r#"using Arc.Collections.Concurrent;

void Main() {
    ConcurrentDictionary<string, string> d = new ConcurrentDictionary<string, string>();
    if (!d.TryAdd("k1", "v1")) { Console.WriteLine("ARC_CASE:dict_keys_snapshot:FAIL:add k1"); return; }
    if (!d.TryAdd("k2", "v2")) { Console.WriteLine("ARC_CASE:dict_keys_snapshot:FAIL:add k2"); return; }
    if (!d.TryAdd("k3", "v3")) { Console.WriteLine("ARC_CASE:dict_keys_snapshot:FAIL:add k3"); return; }
    string[] keys = d.Keys;
    if (keys.Length != 3) { Console.WriteLine("ARC_CASE:dict_keys_snapshot:FAIL:len=" + keys.Length); return; }
    bool hasK1 = false; bool hasK2 = false; bool hasK3 = false;
    for (int i = 0; i < keys.Length; i++) {
        if (keys[i] == "k1") { hasK1 = true; }
        else if (keys[i] == "k2") { hasK2 = true; }
        else if (keys[i] == "k3") { hasK3 = true; }
    }
    if (!hasK1 || !hasK2 || !hasK3) { Console.WriteLine("ARC_CASE:dict_keys_snapshot:FAIL:contents"); return; }
    Console.WriteLine("ARC_CASE:dict_keys_snapshot:PASS");
}
"#,
            ),
        ],
    );
    assert_all_passed("concurrency_collections", &results);
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_concurrency_blocking_batch() {
    let results = assert_compiles_and_runs_batch(
        "concurrency_blocking",
        &[
            (
                "bc_pcc_queue_fifo",
                r#"using Arc.Collections.Concurrent;

void Main() {
    ConcurrentQueue<int> q = new ConcurrentQueue<int>();
    q.Enqueue(10);
    q.Enqueue(20);
    BlockingCollection<int> bc = new BlockingCollection<int>(q, 0);
    int a;
    int b;
    if (!bc.TryTake(out a)) { Console.WriteLine("ARC_CASE:bc_pcc_queue_fifo:FAIL:take a"); return; }
    if (a != 10) { Console.WriteLine("ARC_CASE:bc_pcc_queue_fifo:FAIL:fifo a"); return; }
    if (!bc.TryTake(out b)) { Console.WriteLine("ARC_CASE:bc_pcc_queue_fifo:FAIL:take b"); return; }
    if (b != 20) { Console.WriteLine("ARC_CASE:bc_pcc_queue_fifo:FAIL:fifo b"); return; }
    Console.WriteLine("ARC_CASE:bc_pcc_queue_fifo:PASS");
}
"#,
            ),
            (
                "bc_pcc_stack_lifo",
                r#"using Arc.Collections.Concurrent;

void Main() {
    ConcurrentStack<int> st = new ConcurrentStack<int>();
    st.Push(1);
    st.Push(2);
    BlockingCollection<int> bc = new BlockingCollection<int>(st, 4);
    int a;
    int b;
    if (!bc.TryTake(out a)) { Console.WriteLine("ARC_CASE:bc_pcc_stack_lifo:FAIL:take a"); return; }
    if (a != 2) { Console.WriteLine("ARC_CASE:bc_pcc_stack_lifo:FAIL:lifo a"); return; }
    if (!bc.TryTake(out b)) { Console.WriteLine("ARC_CASE:bc_pcc_stack_lifo:FAIL:take b"); return; }
    if (b != 1) { Console.WriteLine("ARC_CASE:bc_pcc_stack_lifo:FAIL:lifo b"); return; }
    Console.WriteLine("ARC_CASE:bc_pcc_stack_lifo:PASS");
}
"#,
            ),
            (
                "bc_pcc_bag_bounded",
                r#"using Arc.Collections.Concurrent;

void Main() {
    ConcurrentBag<int> bag = new ConcurrentBag<int>();
    bag.Add(7);
    BlockingCollection<int> bc = new BlockingCollection<int>(bag, 2);
    if (!bc.TryAdd(8)) { Console.WriteLine("ARC_CASE:bc_pcc_bag_bounded:FAIL:add 8"); return; }
    if (bc.TryAdd(9)) { Console.WriteLine("ARC_CASE:bc_pcc_bag_bounded:FAIL:bounded full"); return; }
    int v;
    if (!bc.TryTake(out v)) { Console.WriteLine("ARC_CASE:bc_pcc_bag_bounded:FAIL:take"); return; }
    if (!bc.TryAdd(9)) { Console.WriteLine("ARC_CASE:bc_pcc_bag_bounded:FAIL:add after take"); return; }
    Console.WriteLine("ARC_CASE:bc_pcc_bag_bounded:PASS");
}
"#,
            ),
            (
                "bc_bounded_capacity",
                r#"using Arc.Collections.Concurrent;

void Main() {
    BlockingCollection<int> bounded = new BlockingCollection<int>(3);
    if (bounded.BoundedCapacity != 3) { Console.WriteLine("ARC_CASE:bc_bounded_capacity:FAIL:bounded cap=" + bounded.BoundedCapacity); return; }
    BlockingCollection<int> unbounded = new BlockingCollection<int>(0);
    if (unbounded.BoundedCapacity != 2147483647) { Console.WriteLine("ARC_CASE:bc_bounded_capacity:FAIL:unbounded cap=" + unbounded.BoundedCapacity); return; }
    Console.WriteLine("ARC_CASE:bc_bounded_capacity:PASS");
}
"#,
            ),
            (
                "bc_to_array",
                r#"using Arc.Collections.Concurrent;

void Main() {
    BlockingCollection<string> bc = new BlockingCollection<string>(0);
    bc.Add("a");
    bc.Add("b");
    bc.Add("c");
    string[] arr = bc.ToArray();
    if (arr.Length != 3) { Console.WriteLine("ARC_CASE:bc_to_array:FAIL:len=" + arr.Length); return; }
    if (arr[0] != "a" || arr[1] != "b" || arr[2] != "c") { Console.WriteLine("ARC_CASE:bc_to_array:FAIL:contents"); return; }
    Console.WriteLine("ARC_CASE:bc_to_array:PASS");
}
"#,
            ),
        ],
    );
    assert_all_passed("concurrency_blocking", &results);
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_threadpool_sync_batch() {
    let results = assert_compiles_and_runs_batch(
        "threadpool_sync",
        &[
            (
                "threadpool_sync_shutdown",
                r#"using Arc;
using Arc.Threading;

void Main() {
    var pool = new ThreadPoolScheduler(2, false);
    var t = pool.Run(() => {
        Console.WriteLine("sync body");
    });
    t.Wait(5000);
    if (!t.IsCompleted) { Console.WriteLine("ARC_CASE:threadpool_sync_shutdown:FAIL:not-completed"); return; }
    pool.Shutdown();
    Console.WriteLine("ARC_CASE:threadpool_sync_shutdown:PASS");
}
"#,
            ),
            (
                "threadpool_safe_destroy",
                r#"using Arc;
using Arc.Threading;

void Main() {
    var pool = new ThreadPoolScheduler(2, false);
    var t = pool.Run(() => {
        Console.WriteLine("destroy body");
    });
    t.Wait(5000);
    if (!t.IsCompleted) { Console.WriteLine("ARC_CASE:threadpool_safe_destroy:FAIL:incomplete"); return; }
    pool.Destroy();
    Console.WriteLine("destroy ok");

    var pool2 = new ThreadPoolScheduler(2, false);
    var t2 = pool2.Run(() => {
        Console.WriteLine("shutdown then destroy body");
    });
    t2.Wait(5000);
    pool2.Shutdown();
    pool2.Destroy();
    if (!t2.IsCompleted) { Console.WriteLine("ARC_CASE:threadpool_safe_destroy:FAIL:t2-incomplete"); return; }
    Console.WriteLine("ARC_CASE:threadpool_safe_destroy:PASS");
}
"#,
            ),
            (
                "threadpool_numa_aware",
                r#"using Arc;
using Arc.Threading;

void Main() {
    var pool = new ThreadPoolScheduler(2, true);
    if (pool.ActiveWorkerCount != 2) { Console.WriteLine("ARC_CASE:threadpool_numa_aware:FAIL:workers=" + pool.ActiveWorkerCount); return; }
    var t = pool.Run(() => {
        Console.WriteLine("numa body");
    });
    t.Wait(5000);
    if (!t.IsCompleted) { Console.WriteLine("ARC_CASE:threadpool_numa_aware:FAIL:not-completed"); return; }
    pool.Destroy();
    Console.WriteLine("ARC_CASE:threadpool_numa_aware:PASS");
}
"#,
            ),
            (
                "threadpool_optional_ctor_defaults",
                r#"using Arc;
using Arc.Threading;

void Main() {
    var d = new ThreadPoolScheduler();
    if (d.ActiveWorkerCount <= 0) { Console.WriteLine("ARC_CASE:threadpool_optional_ctor_defaults:FAIL:defaults-workers"); return; }
    d.Destroy();

    var w = new ThreadPoolScheduler(2);
    if (w.ActiveWorkerCount != 2) { Console.WriteLine("ARC_CASE:threadpool_optional_ctor_defaults:FAIL:omit-numa=" + w.ActiveWorkerCount); return; }
    w.Destroy();

    var n = new ThreadPoolScheduler(numaAware: true);
    if (n.ActiveWorkerCount <= 0) { Console.WriteLine("ARC_CASE:threadpool_optional_ctor_defaults:FAIL:named-numa"); return; }
    var t = n.Run(() => {
        Console.WriteLine("named numa body");
    });
    t.Wait(5000);
    if (!t.IsCompleted) { Console.WriteLine("ARC_CASE:threadpool_optional_ctor_defaults:FAIL:named-body"); return; }
    n.Destroy();
    Console.WriteLine("ARC_CASE:threadpool_optional_ctor_defaults:PASS");
}
"#,
            ),
            (
                "threadpool_pressure_many_tasks",
                r#"using Arc;
using Arc.Collections.Concurrent;
using Arc.Threading;

void Main() {
    var pool = new ThreadPoolScheduler(4, false);
    var bag = new ConcurrentBag<int>();
    Task t0 = pool.Run(() => { bag.Add(1); });
    Task t1 = pool.Run(() => { bag.Add(1); });
    Task t2 = pool.Run(() => { bag.Add(1); });
    Task t3 = pool.Run(() => { bag.Add(1); });
    Task t4 = pool.Run(() => { bag.Add(1); });
    Task t5 = pool.Run(() => { bag.Add(1); });
    Task t6 = pool.Run(() => { bag.Add(1); });
    Task t7 = pool.Run(() => { bag.Add(1); });
    Task t8 = pool.Run(() => { bag.Add(1); });
    Task t9 = pool.Run(() => { bag.Add(1); });
    Task t10 = pool.Run(() => { bag.Add(1); });
    Task t11 = pool.Run(() => { bag.Add(1); });
    Task t12 = pool.Run(() => { bag.Add(1); });
    Task t13 = pool.Run(() => { bag.Add(1); });
    Task t14 = pool.Run(() => { bag.Add(1); });
    Task t15 = pool.Run(() => { bag.Add(1); });
    t0.Wait(5000);
    t1.Wait(5000);
    t2.Wait(5000);
    t3.Wait(5000);
    t4.Wait(5000);
    t5.Wait(5000);
    t6.Wait(5000);
    t7.Wait(5000);
    t8.Wait(5000);
    t9.Wait(5000);
    t10.Wait(5000);
    t11.Wait(5000);
    t12.Wait(5000);
    t13.Wait(5000);
    t14.Wait(5000);
    t15.Wait(5000);
    if (bag.Count != 16) { Console.WriteLine("ARC_CASE:threadpool_pressure_many_tasks:FAIL:count=" + bag.Count); return; }
    if (pool.PendingTaskCount != 0) { Console.WriteLine("ARC_CASE:threadpool_pressure_many_tasks:FAIL:pending=" + pool.PendingTaskCount); return; }
    pool.Destroy();
    Console.WriteLine("ARC_CASE:threadpool_pressure_many_tasks:PASS");
}
"#,
            ),
        ],
    );
    assert_all_passed("threadpool_sync", &results);
    let get = |name: &str| {
        results
            .iter()
            .find(|r| r.name == name)
            .expect("case result present")
    };
    assert!(
        get("threadpool_sync_shutdown").stdout.contains("sync body"),
        "sync body missing: {}",
        get("threadpool_sync_shutdown").stdout
    );
    let destroy = get("threadpool_safe_destroy").stdout.clone();
    assert!(
        destroy.contains("destroy body") && destroy.contains("shutdown then destroy body"),
        "destroy bodies missing: {destroy}"
    );
    assert!(
        get("threadpool_numa_aware").stdout.contains("numa body"),
        "numa body missing: {}",
        get("threadpool_numa_aware").stdout
    );
    assert!(
        get("threadpool_optional_ctor_defaults")
            .stdout
            .contains("named numa body"),
        "named numa body missing: {}",
        get("threadpool_optional_ctor_defaults").stdout
    );
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_threadpool_async_batch() {
    let results = assert_compiles_and_runs_batch(
        "threadpool_async",
        &[
            (
                "threadpool_pool_run",
                r#"using Arc;
using Arc.Collections.Concurrent;
using Arc.Threading;

async Task<void> Main() {
    var pool = new ThreadPoolScheduler(2, false);
    var hits = new ConcurrentQueue<int>();
    await pool.Run(() => {
        hits.Enqueue(1);
        Console.WriteLine("pool run body");
    });
    if (hits.Count != 1) { Console.WriteLine("ARC_CASE:threadpool_pool_run:FAIL:body"); return; }
    if (pool.ActiveWorkerCount != 2) { Console.WriteLine("ARC_CASE:threadpool_pool_run:FAIL:workers=" + pool.ActiveWorkerCount); return; }
    if (pool.PendingTaskCount != 0) { Console.WriteLine("ARC_CASE:threadpool_pool_run:FAIL:pending=" + pool.PendingTaskCount); return; }
    pool.Shutdown();
    Console.WriteLine("ARC_CASE:threadpool_pool_run:PASS");
}
"#,
            ),
            (
                "threadpool_task_run_on_pool",
                r#"using Arc;
using Arc.Threading;

async Task<void> Main() {
    var pool = new ThreadPoolScheduler(3, false);
    await Task.Run(() => {
        Console.WriteLine("task run on pool");
    }, pool);
    if (pool.ActiveWorkerCount != 3) { Console.WriteLine("ARC_CASE:threadpool_task_run_on_pool:FAIL:workers=" + pool.ActiveWorkerCount); return; }
    pool.Shutdown();
    Console.WriteLine("ARC_CASE:threadpool_task_run_on_pool:PASS");
}
"#,
            ),
            (
                "threadpool_preempt_await_path",
                r#"using Arc;
using Arc.Diagnostics;
using Arc.Threading;

async Task<void> Main() {
    var pool = new ThreadPoolScheduler(2, false);
    await pool.Run(() => {
        Console.WriteLine("preempt pool body");
    });
    Stopwatch sw = Stopwatch.StartNew();
    await Task.Delay(20);
    sw.Stop();
    if (sw.ElapsedMilliseconds < 15) { Console.WriteLine("ARC_CASE:threadpool_preempt_await_path:FAIL:delay=" + sw.ElapsedMilliseconds); return; }
    Console.WriteLine("preempt delay ok");
    pool.Shutdown();
    Console.WriteLine("ARC_CASE:threadpool_preempt_await_path:PASS");
}
"#,
            ),
        ],
    );
    assert_all_passed("threadpool_async", &results);
    let get = |name: &str| {
        results
            .iter()
            .find(|r| r.name == name)
            .expect("case result present")
    };
    assert!(
        get("threadpool_pool_run").stdout.contains("pool run body"),
        "pool run body missing: {}",
        get("threadpool_pool_run").stdout
    );
    assert!(
        get("threadpool_task_run_on_pool")
            .stdout
            .contains("task run on pool"),
        "task run on pool missing: {}",
        get("threadpool_task_run_on_pool").stdout
    );
    let preempt = get("threadpool_preempt_await_path").stdout.clone();
    assert!(
        preempt.contains("preempt pool body") && preempt.contains("preempt delay ok"),
        "preempt path output missing: {preempt}"
    );
}

/// threadpool_scheduler / cycle_collection_thread e2e 的最后一段：循环收集器
/// 线程安全（RFC 005 milestone ①，TLS / per-thread 候选队列）。三场景合并为
/// 一个同步批（无 async Main）：
///   cycle_per_thread_collect：4 线程各自 churn Plain 对象（不可成环、仅并发
///     dec 压力）+ 构造/断链纯环 + `rt_arc_collect` → Weak 观察者全部置空，
///     每线程打印 `t-collected`（Rust 侧计数 ==4）；任一线程未收集即打
///     FAIL 标记。
///   cycle_shared_concurrent_dec：B1 共享不可成环对象并发读值 + 并发 dec →
///     `shared-dec-ok`（值损坏 / rc 未归零打 FAIL）；B2 共享可成环 hub 并发
///     挂链后各自收集 → `shared-cycle-reclaimed` 或 `shared-cycle-leaked`
///     均为文档化合法姿态（仅断言无 AV/DF，即批进程不崩）。
///   cycle_cross_thread_leak：Main 建 X→Y、worker 建 Y→X，各自在本线程断链
///     → per-thread 队列 + candidate pin 使跨线程环不收集（RFC 005 §2.5
///     文档化泄漏姿态）→ `cross-thread-cycle-leaked` 必现；被回收打 FAIL。
/// 场景 A/C 的顶层类原名同为 `Holder`，合并为单文件批后重命名 HolderA /
/// HolderC。`arc_test.rt_arc_collect()` 由开发态内建契约
/// crates/arc/native/arc_test.ani 提供（loader 自动发现），无需 extra_deps。
/// 三 case 均保留原 e2e 的 `setter-return-fail` 反断言（Rust 侧逐 case 检查）。
#[cfg(feature = "full-rt")]
#[test]
fn runs_cycle_collection_batch() {
    let results = assert_compiles_and_runs_batch(
        "cycle_collection",
        &[
            (
                "cycle_per_thread_collect",
                r#"using Arc;
using Arc.Threading;

class A
{
    public B B;
    public int Id;
    public A(int id) { Id = id; }
}

class B
{
    public A A;
    public int Id;
    public B(int id) { Id = id; }
}

class HolderA
{
    public Weak<A> Wa;
    public Weak<B> Wb;
}

class Plain
{
    public int Id;
    public Plain(int id) { Id = id; }
}

void BuildCycle(HolderA h)
{
    A a = new A(1);
    B b = new B(2);
    a.B = b;
    b.A = a;
    h.Wa = new Weak<A>(a);
    h.Wb = new Weak<B>(b);
}

void Worker(int churn)
{
    for (int i = 0; i < churn; i++) {
        Plain p = new Plain(i);
        p.Id = p.Id + 1;
    }
    HolderA h = new HolderA();
    BuildCycle(h);
    arc_test.rt_arc_collect();
    A afterA = h.Wa.TryGet();
    B afterB = h.Wb.TryGet();
    if (afterA == null && afterB == null) {
        Console.WriteLine("t-collected");
    } else {
        Console.WriteLine("ARC_CASE:cycle_per_thread_collect:FAIL:t-not-collected");
    }
}

void Main()
{
    int churn = 2000;
    Thread t0 = new Thread(() => { Worker(churn); });
    Thread t1 = new Thread(() => { Worker(churn); });
    Thread t2 = new Thread(() => { Worker(churn); });
    Thread t3 = new Thread(() => { Worker(churn); });
    t0.Start();
    t1.Start();
    t2.Start();
    t3.Start();
    t0.Join();
    t1.Join();
    t2.Join();
    t3.Join();
    Console.WriteLine("ARC_CASE:cycle_per_thread_collect:PASS");
}
"#,
            ),
            (
                "cycle_shared_concurrent_dec",
                r#"using Arc;
using Arc.Threading;

class Shared
{
    public int Value;
    public Shared(int v) { Value = v; }
}

class Node
{
    public Node Other;
    public int Id;
    public Node(int id) { Id = id; }
}

void Main()
{
    Shared s = new Shared(7);
    Weak<Shared> w = new Weak<Shared>(s);
    Thread s0 = new Thread(() => { if (s.Value != 7) Console.WriteLine("ARC_CASE:cycle_shared_concurrent_dec:FAIL:shared-value-corrupt"); });
    Thread s1 = new Thread(() => { if (s.Value != 7) Console.WriteLine("ARC_CASE:cycle_shared_concurrent_dec:FAIL:shared-value-corrupt"); });
    Thread s2 = new Thread(() => { if (s.Value != 7) Console.WriteLine("ARC_CASE:cycle_shared_concurrent_dec:FAIL:shared-value-corrupt"); });
    Thread s3 = new Thread(() => { if (s.Value != 7) Console.WriteLine("ARC_CASE:cycle_shared_concurrent_dec:FAIL:shared-value-corrupt"); });
    s0.Start();
    s1.Start();
    s2.Start();
    s3.Start();
    s0.Join();
    s1.Join();
    s2.Join();
    s3.Join();
    s = null;
    Shared after = w.TryGet();
    if (after != null) { Console.WriteLine("ARC_CASE:cycle_shared_concurrent_dec:FAIL:shared-dec-rc-not-zero"); return; }
    Console.WriteLine("shared-dec-ok");

    Node hub = new Node(99);
    Weak<Node> wh = new Weak<Node>(hub);
    Thread n0 = new Thread(() => { Node p = new Node(0); p.Other = hub; p = null; arc_test.rt_arc_collect(); });
    Thread n1 = new Thread(() => { Node p = new Node(1); p.Other = hub; p = null; arc_test.rt_arc_collect(); });
    Thread n2 = new Thread(() => { Node p = new Node(2); p.Other = hub; p = null; arc_test.rt_arc_collect(); });
    Thread n3 = new Thread(() => { Node p = new Node(3); p.Other = hub; p = null; arc_test.rt_arc_collect(); });
    n0.Start();
    n1.Start();
    n2.Start();
    n3.Start();
    n0.Join();
    n1.Join();
    n2.Join();
    n3.Join();
    hub = null;
    arc_test.rt_arc_collect();
    Node afterHub = wh.TryGet();
    if (afterHub == null) {
        Console.WriteLine("shared-cycle-reclaimed");
    } else {
        Console.WriteLine("shared-cycle-leaked");
    }
    Console.WriteLine("ARC_CASE:cycle_shared_concurrent_dec:PASS");
}
"#,
            ),
            (
                "cycle_cross_thread_leak",
                r#"using Arc;
using Arc.Threading;

class X
{
    public Y Y;
    public int Id;
    public X(int id) { Id = id; }
}

class Y
{
    public X X;
    public int Id;
    public Y(int id) { Id = id; }
}

class HolderC
{
    public Weak<X> Wx;
    public Weak<Y> Wy;
    public X RefX;
    public Y RefY;
}

void Main()
{
    HolderC h = new HolderC();
    X x = new X(1);
    Y y = new Y(2);
    x.Y = y;
    h.RefX = x;
    h.RefY = y;
    h.Wx = new Weak<X>(x);
    h.Wy = new Weak<Y>(y);

    Thread t = new Thread(() => {
        X xx = h.RefX;
        Y yy = h.RefY;
        yy.X = xx;
    });
    t.Start();
    t.Join();

    h.RefX = null;
    h.RefY = null;

    arc_test.rt_arc_collect();

    X afterX = h.Wx.TryGet();
    Y afterY = h.Wy.TryGet();
    if (afterX != null && afterY != null) {
        Console.WriteLine("cross-thread-cycle-leaked");
    } else {
        Console.WriteLine("ARC_CASE:cycle_cross_thread_leak:FAIL:cross-thread-cycle-reclaimed");
        return;
    }
    Console.WriteLine("ARC_CASE:cycle_cross_thread_leak:PASS");
}
"#,
            ),
        ],
    );
    assert_all_passed("cycle_collection", &results);
    let get = |name: &str| {
        results
            .iter()
            .find(|r| r.name == name)
            .expect("case result present")
    };
    let per_thread = get("cycle_per_thread_collect");
    assert_eq!(
        per_thread.stdout.matches("t-collected").count(),
        4,
        "expected all 4 worker threads to collect their own cycles; stdout: {}",
        per_thread.stdout
    );
    assert!(
        !per_thread.stdout.contains("t-not-collected"),
        "a worker reported a cycle not collected; stdout: {}",
        per_thread.stdout
    );
    let shared = get("cycle_shared_concurrent_dec").stdout.clone();
    assert!(
        shared.contains("shared-dec-ok"),
        "shared object refcount did not reach 0 after concurrent dec; stdout: {shared}"
    );
    assert!(
        shared.contains("shared-cycle-reclaimed") || shared.contains("shared-cycle-leaked"),
        "expected a shared-cycle outcome line; stdout: {shared}"
    );
    assert!(
        get("cycle_cross_thread_leak")
            .stdout
            .contains("cross-thread-cycle-leaked"),
        "expected the cross-thread cycle to leak (documented TLS posture); stdout: {}",
        get("cycle_cross_thread_leak").stdout
    );
    for r in &results {
        assert!(
            !r.stdout.contains("setter-return-fail"),
            "setter return mismatch in {}: {}",
            r.name,
            r.stdout
        );
    }
}

#[cfg(not(feature = "full-rt"))]
#[test]
fn runs_concurrency_core_batch() {
    // L2 runtime tests require --features full-rt
}

#[cfg(not(feature = "full-rt"))]
#[test]
fn runs_concurrency_collections_batch() {
    // L2 runtime tests require --features full-rt
}

#[cfg(not(feature = "full-rt"))]
#[test]
fn runs_concurrency_blocking_batch() {
    // L2 runtime tests require --features full-rt
}
