# RFC 009 异步与并发模型

## 背景

Arc 异步体系**底层直接对标 LLVM 协程原语**（coroutine intrinsics），**表面提供 C# Task 体系的优雅与简洁**：用户写 `async`/`await` 与 `Task<T>`，编译器将 async 函数发射为 LLVM 协程，由 LLVM Coro 优化管线完成协程帧分裂、spill 与 elision。**默认多线程 executor**（RFC 009 M6）：EventLoop 驱动线程驱动 IO/定时器，协程续体由线程池（worker=硬件并发）并行执行；协作式取消；显式线程原语。目标：异步零成本、行为确定、性能可证伪。

## 设计决策

### 异步函数

```as
async Task<int> fetchValue() {
    return 42;
}

async Task<void> main() {
    var v = await fetchValue();
    Console.WriteLine("done");
}
```

规则：
1. 含 `await` 的函数必须标记 `async`。
2. 返回类型必须为 `Task<T>` 或 `Task<void>`。
3. `await` 仅作用于 `Task<T>`（编译器内建），结果为 `T`；无第三方可等待类型协议。

### `Task<T>` 语义

`Task<T>` 表示异步计算的句柄：

| 状态 | 含义 |
|------|------|
| Ready | 已完成，可读取结果 |
| Pending | 等待外部事件（定时器 / 组合子 / IO） |
| Faulted | 异常终止 |
| Canceled | 协作式取消完成 |

运行时结构（C ABI `RtTask`）：`status` + `int_result` + `ptr_result` + `resume` / `destroy` 函数指针对（直接对应 LLVM 协程的 resume/destroy 句柄）+ `resume_data`（协程帧指针）；`rt_task_poll` 驱动状态推进。

#### 结果所有权（强持有模型）

Task 的指针结果槽（`ptr_result`）由 Task **强持有**：

| 路径 | 所有权语义 |
|------|-----------|
| 协程返回值 | 返回值的 +1 由协程帧持有并转入 task（`rt_task_from_ptr` 接管所有权） |
| `rt_task_release` | 统一 dec `ptr_result`（FAULTED 时释放异常对象——异常所有权随 Task 终止） |
| `await` 提取 / `task.Result` / `GetResult()` 同步提取 | 对 class 结果 retain——返回强持有引用，与调用方局部出口 dec 配对 |
| `Task.FromResult(class)` | 拦截器 inc 后入 task（task 持 +1） |

string / array / Task / Func 结果无 ArcHeader，任何路径都不 retain。

### 协程 lowering（LLVM coroutine intrinsics）

| 阶段 | 职责 |
|------|------|
| `typeck` | 验证 `async`/`await` 类型（返回 `Task<T>`/`Task<void>`、`await` 操作数类型） |
| `mir` | 保留 async 函数**完整 CFG**（不拆状态机）；每个 `await` 边界标注 suspend 点 |
| `codegen` | 发射 LLVM 协程原语：`llvm.coro.id` + `llvm.coro.begin` 分配协程帧；await 边界发射 `llvm.coro.suspend`；函数出口发射 `llvm.coro.end` |
| LLVM Coro 管线 | `CoroEarly` / `CoroSplit` / `CoroElide` 将协程分裂为 resume/destroy 两半，跨 await 存活局部自动 spill 进协程帧 |

要点：

1. **codegen 不手工拆 CFG 状态机**——协程帧布局、resume/destroy 分裂、跨 suspend 局部 spill 全部由 LLVM Coro 优化管线完成，编译器只负责发射 `coro.*` intrinsic 与 Task 句柄桥接。
2. `RtTask.resume` / `RtTask.destroy` 直接绑定 CoroSplit 产出的两个句柄；`rt_task_poll` 调用 `resume`，`rt_task_release` 调用 `destroy`（帧内存随协程帧释放）。
3. 无 await 的 async 走同步构造路径（CoroElide 直接内联，零协程帧开销）。
4. 协程帧按需分配：`llvm.coro.size` 精确帧尺寸，无冗余堆槽。

### 唤醒

`rt_waker_wake` 将任务投递至续体执行器：绑定线程池后经 `g_rt_wake_fn` 重定向为向线程池投递 poll-task（多线程并行执行，见「调度器」）；未绑定回退单线程就绪队列。定时器到期与组合子 inner 完成走同一链路。`async Main` 由 codegen 包装为 EventLoop create → reactor create → 线程池绑定 → spawn → run → destroy。

### Task API

| API | 语义 |
|-----|------|
| `Task.FromResult` / `CompletedTask` | 已完成任务 |
| `Task.Delay` | 定时延迟（EventLoop suspend/resume） |
| `WhenAll` / `WhenAny` | 组合子（`params ReadOnlySpan<Task>`） |
| `WaitAll` / `WaitAny` | 同步等待 |
| `Task.Run(Action)` / `Task.Run<T>(Func<T>)` | 默认线程池执行 |

延续经 `await` 组合表达（单一惯用法）；外部事件到 Task 的桥接由运行时/std 内部承接（`rt_task_*`）。

### 调度器（RFC 009 M6：多线程 Executor）

**默认多线程策略**：async 程序默认形态为「EventLoop 驱动线程 + 线程池续体执行器」——

| 组件 | 职责 |
|------|------|
| EventLoop（单驱动线程） | Reactor(IO 多路复用) + 定时器（3 级时间轮）+ 就绪检测 + 退出判定；**不自 poll 续体** |
| ThreadPool（N worker = 硬件并发） | 协程续体并行执行：poll/resume/推进状态机；Chase-Lev work-stealing 负载均衡 |

任务分发链路：

- `async Main` 由 codegen 发射：EventLoop create → reactor create → 线程池 create（worker=hardware_concurrency）→ `rt_event_loop_set_threadpool` 绑定 → spawn root → run。
- **wake 重定向**：绑定后 `g_rt_wake_fn` 指向 `rt_task_threadpool_wake`——任意线程 wake 就绪 Task 即向线程池投递 poll-task 工作项（非 worker 线程走全局无锁 injector，空闲 worker 拉取；worker 线程走私有 deque/LIFO）。
- **EventLoop tick 只转投不执行**：就绪 Task 投递线程池后驱动线程阻塞于 `reactor_poll`（IO/定时器）；根任务完成由 worker 跨线程唤醒（IOCP `PostQueuedCompletionStatus` / kqueue `EVFILT_USER` 哨兵；io_uring/poll 预留，≤100ms 轮询兜底），及时检查退出。

多线程安全（同一 Task 的并发 poll）：

- `RtTask.poll_flags` 的 POLLING/NOTIFIED 两位 CAS 守卫——某 worker 抢占 POLLING 后其余 poll 置 NOTIFIED 返回 PENDING；持锁线程释放时见位清位并重 poll 一次，闭合「poll 中唤醒」竞态（防 resume 重入 + 防丢失唤醒）。
- 根任务完成由 worker 收口 `pending_count`（`root_task` 精确匹配，嵌套子任务不误减）并唤醒驱动线程。

**单线程回退**：`rt_event_loop_set_threadpool(loop, NULL)` 解绑即回退 M1–M5 行为（EventLoop 自 poll）——确定性调试 / 资源受限场景显式选择。

调度器以**最简稳定**为先，调度复杂度不构成当前性能瓶颈。

### 取消

- `CancellationToken` / `CancellationTokenSource`（Cancel / CancelAfter / Register）。
- `cancellationToken.ThrowIfCancellationRequested()` 协作式抛取消。
- 取消为**可选**协作机制：需要取消的 API 显式接受 `CancellationToken`（std 异步 IO 惯例）；**不强制**所有 async 方法携带 CT 参数。

### 线程原语与并发

| 原语 | 说明 |
|------|------|
| `Thread` | 显式线程 |
| `Mutex` / `Semaphore` | 互斥与信号量 |
| `Monitor` / `Lock` | 监视器锁 |
| `lock` 语句糖 | `lock (o) { ... }` → `Monitor.Enter/Exit` + try/finally |
| `Interlocked` | 原子操作 |
| `Concurrent*` 集合 | 并发容器（见 [并发集合](024-concurrent-collections.md)） |

底层分工：LLVM IR **不提供**线程创建与高级并发抽象，只提供原子指令（`cmpxchg` / `atomicrmw` + 内存顺序 monotonic / acquire / release / seq_cst）与协程 intrinsics（`llvm.coro.*`）两类机制。线程创建由运行时（`rt_thread_*`）封装 OS API；并发容器在 std 基于原子指令实现（`ConcurrentDictionary` 等无锁算法经 codegen 发射 `cmpxchg`/`atomicrmw`）；异步迭代器（`await foreach`）编译为协程 resume 调用循环。**编译器只发射机制，不内置数据结构**——高级抽象全部由 std/运行时构建（架构红线）。

### 与所有权的交互

- `await` 期间，已移动进协程帧的变量遵循 borrowck 在 suspend 边界的分析。
- 跨 await 可变借用默认禁止，除非实现证明安全。
- 跨 await 存活局部由 LLVM 协程帧 spill 承载（`llvm.coro.size` 精确尺寸）。

### 确定性

**默认多线程策略下的确定性契约**：续体执行**不保证线程亲缘确定性**——同一 Task 的两次续体可能由不同 worker 执行，await 恢复顺序不保证与代码字面一致。**保证的确定性**：

- **因果序（causal order）**：`await` 恢复严格发生在被 await Task 完成之后（wake 经 poll-task 投递，poll 守卫保证完成状态由唯一持锁线程读取）；单一 Task 内部状态推进串行化（POLLING/NOTIFIED 守卫防重入）。
- **同步边界确定**：`task.Result` / `WaitAll` / `WaitAny` 等同步等待语义不依赖调度线程。
- **可复现性选择**：需要线程级确定复现（调试 / 回归快照）时显式解绑线程池（`rt_event_loop_set_threadpool(loop, NULL)`）回退单线程，行为与 M1–M5 一致。

## 边界

- 运行时 `rt_task_*` / `rt_event_loop_*` 符号面见 [运行时 ABI](014-runtime-abi.md)。
- 异步委托与 `AsyncStream` 见 [委托、闭包与方法组](008-delegates-closures.md)。
- 并发集合类型库见 [并发集合](024-concurrent-collections.md)。

## 禁止项

- **不引入 C# `ValueTask` 多形**（单一惯用法）。
- **不引入 `ContinueWith` / `TaskCompletionSource` / Awaiter 模式**（`await` 仅作用于编译器内建的 `Task<T>`；延续经 `await` 组合表达）。
- **不引入协作式抢占检查**（续体调度由线程池承载，协程内不做时间片抢占）。
- **不在 codegen 手工拆 CFG 状态机**（协程分裂由 LLVM Coro 管线承担，编译器只发射 `coro.*` intrinsic）。

---

上一节：[008 委托、闭包与方法组](008-delegates-closures.md) · 下一节：[010 异常与资源管理](010-exceptions-resources.md)
