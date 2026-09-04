# 08 异步与任务

Arc 异步模型基于 **`async`/`await`** 与 **`Task<T>`** 类型。编译器将异步函数 lowering 为状态机，运行时通过 `rt_task_*` ABI 驱动（见[运行时 ABI](12-runtime-abi.md)）。

## 异步函数

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

1. 含 `await` 的函数必须标记 `async`
2. 返回类型必须为 `Task<T>` 或 `Task<void>`
3. `await` 仅作用于 `Task<T>`；结果为 `T`

## `Task<T>` 语义

`Task<T>` 表示异步计算的句柄：

| 状态 | 含义 |
|------|------|
| Ready | 已完成，可读取结果 |
| Pending | 等待外部事件（定时器 / 组合子 / IO） |
| Faulted | 异常终止 |
| Canceled | 协作式取消完成 |

运行时结构（C ABI）：

```c
typedef struct RtTask {
    int32_t status;
    int32_t int_result;
    void (*resume)(struct RtTask*);
    void* resume_data;
} RtTask;
```

`rt_task_poll` 驱动状态推进；`rt_task_result_int` 读取整型结果（类型扩展随泛型单态化）。

## 状态机 lowering

编译管线：

1. `typeck` 验证 `async`/`await` 类型
2. `mir` 将 async 函数拆为状态机基本块
3. `codegen` 生成 poll/resume 调用

异步函数体中的局部变量提升为状态机字段；每个 `await` 边界为一个 suspend 点。
含 await 的 async 经 codegen **整图 CFG** 状态机 lowering（多块链 / 循环内 / 分支臂均覆盖）；无 await 的 async 走同步构造路径。

## 唤醒

`rt_waker_wake` 将任务移入 EventLoop 就绪队列；定时器到期与组合子 inner 完成走同一链路。`async Main` 由 codegen 包装为 EventLoop create → spawn → run → destroy。

## Stable 面

| API / 契约 | 说明 |
|------------|------|
| `await` / `Task.FromResult` / `CompletedTask` | 已完成 Task 的创建与等待 |
| `Task.Delay` + EventLoop suspend/resume | 定时器驱动的挂起/恢复 |
| QIF `[Fact] async`（宿主 `await`） | 测试宿主自动 await |
| `WhenAll` / `WhenAny` / `WaitAll` / `WaitAny`（`params ReadOnlySpan<Task>`） | 任务组合子 |
| `CancellationToken` / CTS Cancel·CancelAfter·Register | 协作式取消 |
| `Task.Run(Action)` / `Task.Run<T>(Func<T>)`（默认线程池） | 后台任务 |
| 显式 `ThreadPoolScheduler` 基本 API（ctor / `Run` / `Task.Run(Action, pool)` / `ActiveWorkerCount` / `PendingTaskCount` / `Shutdown`） | 线程池调度器 |
| `ThreadPoolScheduler` **Destroy**（安全 destroy：wait_idle + join + free；可接 Shutdown） | 安全销毁 |
| 多任务压力最小面（并发完成计数） | 并发正确性 |

## 与所有权的交互

- `await` 期间，已移动进状态机的变量遵循 borrowck 在 suspend 边界的分析
- 跨 await 可变借用默认禁止，除非实现证明安全

## 标准库

| 模块 | 职责 |
|------|------|
| `std/Arc/Tasks/Task.as` | Task facade（codegen → `rt_task_*`） |
| `std/Arc/Tasks/CancellationToken*.as` | 协作式取消 |
| EventLoop | 运行时 `rt_event_loop_*`（无独立 std 类；由 async Main / Delay 隐式驱动） |

## 确定性

异步调度不引入全局隐式线程池：任务何时 poll 由显式运行时与事件循环决定，符合 Arc「行为在编译期确定」信条。

---

上一节：[07 对象模型](07-object-model.md) · 下一节：[09 查询语言](09-query-language.md)